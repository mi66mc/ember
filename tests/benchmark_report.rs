use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[test]
fn comparison_help_and_minimal_report_follow_the_public_contract() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let help = Command::new("python")
        .args(["bench/compare.py", "--help"])
        .current_dir(&manifest_dir)
        .output()
        .expect("Python must be available to show comparison help");
    assert!(
        help.status.success(),
        "comparison help failed: {}",
        String::from_utf8_lossy(&help.stderr)
    );
    let help = String::from_utf8(help.stdout).expect("comparison help must be UTF-8");
    for option in [
        "--ember",
        "--warmup",
        "--samples",
        "--output",
        "--timeout-seconds",
        "--label",
    ] {
        assert!(help.contains(option), "comparison help is missing {option}");
    }

    let minimal_report = r#"{
  "schema_version": 1,
  "environment": {},
  "configuration": {},
  "results": []
}"#;
    let mut parser = Command::new("python")
        .args([
            "-c",
            "import json, sys; report = json.load(sys.stdin); required = {'schema_version', 'environment', 'configuration', 'results'}; missing = required - report.keys(); assert not missing, missing",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .expect("Python must be available to parse a benchmark report");
    parser
        .stdin
        .as_mut()
        .expect("parser stdin must be available")
        .write_all(minimal_report.as_bytes())
        .expect("minimal report must be written to the parser");
    let status = parser.wait().expect("parser must finish");
    assert!(status.success(), "minimal report must satisfy the schema");
}
