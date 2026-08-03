use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
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

fn select_python(
    configured: Option<PathBuf>,
    candidates: &[PathBuf],
    mut is_supported: impl FnMut(&Path) -> bool,
) -> Result<PathBuf, String> {
    if let Some(configured) = configured {
        if is_supported(&configured) {
            return Ok(configured);
        }
        return Err(format!(
            "configured Python is not Python 3.10+: {}",
            configured.display()
        ));
    }
    candidates
        .iter()
        .find(|candidate| is_supported(candidate))
        .cloned()
        .ok_or_else(|| "could not find Python 3.10+; set EMBER_BENCH_PYTHON".to_owned())
}

fn python_executable() -> &'static Path {
    static PYTHON: OnceLock<PathBuf> = OnceLock::new();
    PYTHON
        .get_or_init(|| {
            let configured = std::env::var_os("EMBER_BENCH_PYTHON")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
            #[cfg(windows)]
            let candidates = [PathBuf::from("python"), PathBuf::from("python3")];
            #[cfg(not(windows))]
            let candidates = [PathBuf::from("python3"), PathBuf::from("python")];
            select_python(configured, &candidates, python_is_supported)
                .unwrap_or_else(|error| panic!("{error}"))
        })
        .as_path()
}

fn python_is_supported(executable: &Path) -> bool {
    Command::new(executable)
        .args([
            "-c",
            "import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)",
        ])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn python_selection_prefers_the_configured_executable() {
    let configured = PathBuf::from("configured-python");
    let candidates = [PathBuf::from("python3"), PathBuf::from("python")];
    let mut probed = Vec::new();

    let selected = select_python(Some(configured.clone()), &candidates, |candidate| {
        probed.push(candidate.to_path_buf());
        candidate == configured
    })
    .expect("configured Python should be selected");

    assert_eq!(selected, configured);
    assert_eq!(probed, [configured]);
}

#[test]
fn python_selection_discovers_the_first_supported_candidate() {
    let candidates = [
        PathBuf::from("unsupported-python"),
        PathBuf::from("python-3.10-or-newer"),
        PathBuf::from("later-python"),
    ];
    let mut probed = Vec::new();

    let selected = select_python(None, &candidates, |candidate| {
        probed.push(candidate.to_path_buf());
        candidate == Path::new("python-3.10-or-newer")
    })
    .expect("a supported Python candidate should be discovered");

    assert_eq!(selected, Path::new("python-3.10-or-newer"));
    assert_eq!(probed, candidates[..2]);
}

#[test]
fn fake_ember_launcher_uses_the_selected_python_executable() {
    let temporary = TestDir::new("selected-python-launcher");
    let marker = temporary.path().join("selected-python-ran");

    #[cfg(windows)]
    let selected_python = {
        let launcher = temporary.path().join("selected-python.cmd");
        fs::write(
            &launcher,
            format!(
                "@echo selected>\"{}\"\r\n@echo 832040\r\n",
                marker.display()
            ),
        )
        .expect("selected Python launcher must be writable");
        launcher
    };

    #[cfg(unix)]
    let selected_python = {
        use std::os::unix::fs::PermissionsExt;

        let launcher = temporary.path().join("selected-python");
        fs::write(
            &launcher,
            format!(
                "#!/bin/sh\nprintf selected > '{}'\nprintf '832040\\n'\n",
                marker.display()
            ),
        )
        .expect("selected Python launcher must be writable");
        let mut permissions = fs::metadata(&launcher)
            .expect("selected Python launcher metadata must be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&launcher, permissions)
            .expect("selected Python launcher executable bit must be set");
        launcher
    };

    let fake_ember = create_fake_ember(temporary.path(), &selected_python);
    let output = Command::new(&fake_ember)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {}: {error}", fake_ember.display()));

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "832040");
    assert!(marker.is_file(), "fake Ember bypassed the selected Python");
}

fn create_fake_ember(directory: &Path, python: &Path) -> PathBuf {
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
            format!("@\"{}\" \"{}\" %*\r\n", python.display(), script.display()),
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
                "#!/bin/sh\nexec '{}' '{}' \"$@\"\n",
                python.display(),
                script.display()
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

fn run_comparison(
    python: &Path,
    fake_ember: &Path,
    output_dir: &Path,
    mode: &str,
    timeout: u64,
) -> Output {
    Command::new(python)
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
fn comparison_imports_with_the_python_3_10_datetime_api() {
    let compatibility_check = Command::new(python_executable())
        .args([
            "-c",
            r#"import datetime as real_datetime
import runpy
import sys
import types

python_310_datetime = types.ModuleType("datetime")
python_310_datetime.datetime = real_datetime.datetime
python_310_datetime.timezone = real_datetime.timezone
sys.modules["datetime"] = python_310_datetime
runpy.run_path("bench/compare.py", run_name="bench_compare")
"#,
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Python must be available to check Python 3.10 compatibility");

    assert!(
        compatibility_check.status.success(),
        "compare.py used a datetime API unavailable in Python 3.10: {}",
        String::from_utf8_lossy(&compatibility_check.stderr)
    );
}

#[test]
fn comparison_emits_parseable_schema_and_raw_samples() {
    let temporary = TestDir::new("success");
    let python = python_executable();
    let fake_ember = create_fake_ember(temporary.path(), python);
    let output_dir = temporary.path().join("report");

    let comparison = run_comparison(python, &fake_ember, &output_dir, "success", 30);
    assert!(
        comparison.status.success(),
        "comparison failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&comparison.stdout),
        String::from_utf8_lossy(&comparison.stderr)
    );

    let json_path = output_dir.join("latest.json");
    let markdown_path = output_dir.join("latest.md");
    let validator = Command::new(python)
        .args([
            "-c",
            r#"import hashlib
import json
import sys
from pathlib import Path

with open(sys.argv[1], encoding="utf-8") as source:
    report = json.load(source)

assert report["schema_version"] == 1
assert set(report) == {"schema_version", "environment", "configuration", "results"}
assert {"generated_at_utc", "git_commit", "git_dirty", "python_version", "operating_system", "architecture", "cpu_description", "complete", "ember_executable"} <= set(report["environment"])
ember_path = Path(sys.argv[2]).resolve(strict=True)
ember_stat = ember_path.stat()
ember_identity = report["environment"]["ember_executable"]
assert ember_identity == {
    "canonical_path": str(ember_path),
    "size_bytes": ember_stat.st_size,
    "mtime_ns": ember_stat.st_mtime_ns,
    "sha256": hashlib.sha256(ember_path.read_bytes()).hexdigest(),
}
assert Path(ember_identity["canonical_path"]).is_absolute()
assert "git_commit" not in ember_identity
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

markdown = Path(sys.argv[3]).read_text(encoding="utf-8")
assert f"- Canonical path: `{ember_identity['canonical_path']}`" in markdown
assert f"- Size: {ember_identity['size_bytes']} bytes" in markdown
assert f"- Modification time: {ember_identity['mtime_ns']} ns since the Unix epoch" in markdown
assert f"- SHA-256: `{ember_identity['sha256']}`" in markdown
"#,
        ])
        .arg(&json_path)
        .arg(&fake_ember)
        .arg(&markdown_path)
        .output()
        .expect("Python must be available to validate the emitted JSON");
    assert!(
        validator.status.success(),
        "emitted JSON failed validation: {}",
        String::from_utf8_lossy(&validator.stderr)
    );

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
fn comparison_failures_preserve_the_last_successful_reports() {
    let temporary = TestDir::new("failures");
    let python = python_executable();
    let fake_ember = create_fake_ember(temporary.path(), python);
    let output_dir = temporary.path().join("report");

    let successful = run_comparison(python, &fake_ember, &output_dir, "success", 30);
    assert!(
        successful.status.success(),
        "initial comparison failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&successful.stdout),
        String::from_utf8_lossy(&successful.stderr)
    );
    let json_path = output_dir.join("latest.json");
    let markdown_path = output_dir.join("latest.md");
    let successful_json = fs::read(&json_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", json_path.display()));
    let successful_markdown = fs::read(&markdown_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", markdown_path.display()));

    for (mode, timeout, expected_reason) in [
        ("nonzero", 30, "command exited unsuccessfully"),
        ("wrong_output", 30, "command stdout must be exactly 832040"),
        ("timeout", 1, "timed out after 1 seconds"),
    ] {
        let comparison = run_comparison(python, &fake_ember, &output_dir, mode, timeout);
        assert!(
            !comparison.status.success(),
            "{mode} comparison unexpectedly succeeded"
        );
        let stderr = String::from_utf8_lossy(&comparison.stderr);
        assert!(
            stderr.contains("comparison failed; previous latest reports were preserved")
                && stderr.contains(expected_reason),
            "{mode} failure diagnostics were incomplete: {stderr}"
        );
        assert_eq!(
            fs::read(&json_path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", json_path.display())),
            successful_json,
            "{mode} failure changed the last successful JSON report"
        );
        assert_eq!(
            fs::read(&markdown_path).unwrap_or_else(|error| panic!(
                "failed to read {}: {error}",
                markdown_path.display()
            )),
            successful_markdown,
            "{mode} failure changed the last successful Markdown report"
        );
    }
}
