use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ember-benchmark-report-{label}-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)
            .unwrap_or_else(|error| panic!("failed to create {}: {error}", path.display()));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create_fake_ember(directory: &Path) -> PathBuf {
    let script = directory.join("fake_ember.py");
    fs::write(
        &script,
        r#"import os
import sys
import time

mode = os.environ.get("FAKE_EMBER_MODE", "success")
if mode == "timeout":
    time.sleep(3)
elif mode == "nonzero":
    print("synthetic Ember failure", file=sys.stderr)
    raise SystemExit(7)
elif mode == "wrong_output":
    print(42)
else:
    print(832040)
"#,
    )
    .unwrap_or_else(|error| panic!("failed to write {}: {error}", script.display()));

    #[cfg(windows)]
    {
        let launcher = directory.join("fake_ember.cmd");
        fs::write(
            &launcher,
            format!("@python \"{}\" %*\r\n", script.display()),
        )
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", launcher.display()));
        launcher
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let launcher = directory.join("fake_ember");
        fs::write(
            &launcher,
            format!(
                "#!/usr/bin/env python\n{}",
                fs::read_to_string(&script).unwrap()
            ),
        )
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", launcher.display()));
        let mut permissions = fs::metadata(&launcher)
            .expect("fake Ember metadata must be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&launcher, permissions).expect("fake Ember executable bit must be set");
        launcher
    }
}

fn run_comparison(fake_ember: &Path, output_dir: &Path, mode: &str, timeout: u64) -> Output {
    Command::new("python")
        .args(["bench/compare.py", "--ember"])
        .arg(fake_ember)
        .args([
            "--warmup",
            "1",
            "--samples",
            "10",
            "--timeout-seconds",
            &timeout.to_string(),
            "--output",
        ])
        .arg(output_dir)
        .env("FAKE_EMBER_MODE", mode)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Python must be available to run the comparison")
}

#[test]
fn comparison_emits_parseable_schema_and_raw_samples() {
    let temporary = TestDir::new("success");
    let fake_ember = create_fake_ember(temporary.path());
    let output_dir = temporary.path().join("report");

    let comparison = run_comparison(&fake_ember, &output_dir, "success", 30);
    assert!(
        comparison.status.success(),
        "comparison failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&comparison.stdout),
        String::from_utf8_lossy(&comparison.stderr)
    );

    let json_path = output_dir.join("latest.json");
    let validator = Command::new("python")
        .args([
            "-c",
            r#"import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    report = json.load(source)

assert report["schema_version"] == 1
assert set(report) == {"schema_version", "environment", "configuration", "results"}
assert {"generated_at_utc", "git_commit", "git_dirty", "python_version", "operating_system", "architecture", "cpu_description", "complete"} <= set(report["environment"])
assert report["configuration"]["warmup"] == 1
assert report["configuration"]["samples"] == 10
assert report["configuration"]["timeout_seconds"] == 30
assert {(item["workload"], item["runtime"]) for item in report["results"]} == {
    ("fib_inline", "ember"),
    ("fib_inline", "cpython"),
    ("fib_function", "ember"),
    ("fib_function", "cpython"),
}
for item in report["results"]:
    assert len(item["samples_ns"]) == 10
    assert all(isinstance(sample, int) and sample > 0 for sample in item["samples_ns"])
    assert set(item["statistics_ms"]) == {"min_ms", "median_ms", "p95_ms", "max_ms", "mad_ms"}
    assert isinstance(item["command"], list) and item["command"]
"#,
        ])
        .arg(&json_path)
        .output()
        .expect("Python must be available to validate the emitted JSON");
    assert!(
        validator.status.success(),
        "emitted JSON failed validation: {}",
        String::from_utf8_lossy(&validator.stderr)
    );

    let markdown_path = output_dir.join("latest.md");
    let markdown = fs::read_to_string(&markdown_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", markdown_path.display()));
    for row in [
        "| fib_inline | ember | 10 |",
        "| fib_inline | cpython | 10 |",
        "| fib_function | ember | 10 |",
        "| fib_function | cpython | 10 |",
    ] {
        assert!(markdown.contains(row), "Markdown report is missing {row}");
    }
}

#[test]
fn comparison_failures_do_not_publish_latest_reports() {
    let temporary = TestDir::new("failures");
    let fake_ember = create_fake_ember(temporary.path());

    for (mode, timeout, expected_reason) in [
        ("nonzero", 30, "command exited unsuccessfully"),
        ("wrong_output", 30, "command stdout must be exactly 832040"),
        ("timeout", 1, "timed out after 1 seconds"),
    ] {
        let output_dir = temporary.path().join(mode);
        let comparison = run_comparison(&fake_ember, &output_dir, mode, timeout);
        assert!(
            !comparison.status.success(),
            "{mode} comparison unexpectedly succeeded"
        );
        let stderr = String::from_utf8_lossy(&comparison.stderr);
        assert!(
            stderr.contains("comparison failed; no report was published")
                && stderr.contains(expected_reason),
            "{mode} failure diagnostics were incomplete: {stderr}"
        );
        assert!(
            !output_dir.join("latest.json").exists() && !output_dir.join("latest.md").exists(),
            "{mode} failure published a latest report"
        );
    }
}
