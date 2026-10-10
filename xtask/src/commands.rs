//! Real subprocess orchestration for local development and release evidence.
pub(crate) mod process;
mod workflows;
use serde_json::Value;
use std::{
    error::Error,
    ffi::OsString,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, PartialEq, Eq)]
pub enum Task {
    Help,
    Verify,
    Demo(PathBuf),
    InstallSmoke(PathBuf),
    ReleaseCheck,
    BenchReport(PathBuf),
    ReleasePlan,
    ReleaseBuild { target: String, output: PathBuf },
    ReleaseVerify { dir: PathBuf, signatures: bool },
    ReleaseSign(PathBuf),
    DemoRecord(PathBuf),
}
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Task> {
    let args: Vec<_> = args.into_iter().collect();
    let task = match args.as_slice() {
        [] => Task::Help,
        [arg] if arg == "--help" || arg == "-h" => Task::Help,
        [arg] if arg == "verify-repository" => Task::Verify,
        [arg] if arg == "release-plan" => Task::ReleasePlan,
        [task, target_flag, target, output_flag, output]
            if task == "release-build"
                && target_flag == "--target"
                && output_flag == "--output"
                && !target.is_empty()
                && !output.is_empty() =>
        {
            Task::ReleaseBuild {
                target: target.to_str().ok_or("non-UTF8 target")?.to_owned(),
                output: output.into(),
            }
        }
        [task, flag, dir, mode]
            if task == "release-verify"
                && flag == "--dir"
                && !dir.is_empty()
                && (mode == "--integrity-only" || mode == "--require-signatures") =>
        {
            Task::ReleaseVerify {
                dir: dir.into(),
                signatures: mode == "--require-signatures",
            }
        }
        [task, flag, dir, mode, configured]
            if task == "release-sign"
                && flag == "--dir"
                && !dir.is_empty()
                && mode == "--mode"
                && configured == "configured" =>
        {
            Task::ReleaseSign(dir.into())
        }
        [task, flag, output]
            if task == "demo-record" && flag == "--output" && !output.is_empty() =>
        {
            Task::DemoRecord(output.into())
        }
        [task, flag] if task == "release-check" && flag == "--local" => Task::ReleaseCheck,
        [task, flag, path] if !path.is_empty() && task == "demo" && flag == "--record-json" => {
            Task::Demo(path.into())
        }
        [task, flag, path] if !path.is_empty() && task == "install-smoke" && flag == "--root" => {
            Task::InstallSmoke(path.into())
        }
        [task, flag, path] if !path.is_empty() && task == "bench-report" && flag == "--output" => {
            Task::BenchReport(path.into())
        }
        _ => return Err("invalid xtask arguments; use --help".into()),
    };
    Ok(task)
}
pub fn run(root: &Path, args: impl IntoIterator<Item = OsString>) -> Result<()> {
    match parse(args)? {
        Task::Help => println!(
            "Attow Network development tools\n\nUsage: cargo run -p xtask -- COMMAND\n\n  verify-repository\n  demo --record-json PATH\n  install-smoke --root PATH\n  release-check --local\n  bench-report --output PATH\n  release-plan\n  release-build --target TARGET --output DIR\n  release-verify --dir DIR --integrity-only|--require-signatures\n  release-sign --dir DIR --mode configured\n  demo-record --output DIR\n\nDemo and all-features release gates require the pinned browser prerequisites.\nInstall smoke uses a fresh debug source install and an offline data-change run."
        ),
        Task::Verify => {
            crate::verify_repository(root)?;
            println!("Attow Network repository checks passed.");
        }
        Task::Demo(path) => workflows::demo(root, &path)?,
        Task::InstallSmoke(path) => workflows::install_smoke(root, &path)?,
        Task::ReleaseCheck => workflows::release_check(root)?,
        Task::BenchReport(path) => workflows::bench_report(root, &path)?,
        Task::ReleasePlan => println!(
            "{}",
            serde_json::to_string_pretty(&crate::release::build::plan(root)?)?
        ),
        Task::ReleaseBuild { target, output } => {
            crate::release::build::build(root, &target, &output)?
        }
        Task::ReleaseVerify { dir, signatures } => println!(
            "{}",
            serde_json::to_string_pretty(&crate::release::verify::verify_candidate(
                &root.join(dir),
                if signatures {
                    crate::release::verify::VerificationMode::RequireSignatures
                } else {
                    crate::release::verify::VerificationMode::IntegrityOnly
                }
            )?)?
        ),
        Task::ReleaseSign(dir) => println!(
            "{}",
            serde_json::to_string_pretty(&crate::release::sign::sign_configured(&root.join(dir))?)?
        ),
        Task::DemoRecord(output) => {
            let result = crate::release::demo_record::record(root, &root.join(output))?;
            println!("{}", serde_json::to_string_pretty(&result.metadata)?);
        }
    }
    Ok(())
}
fn envelope(bytes: &[u8], code: i32, expected: i32, command: &str) -> Result<Value> {
    if bytes.len() > process::MAX_CAPTURE {
        return Err("CLI output exceeds recording limit".into());
    }
    let value: Value = serde_json::from_slice(bytes)?;
    let status = match expected {
        0 => "success",
        3 => "unknown",
        _ => return Err("invalid expected CLI exit".into()),
    };
    if code != expected
        || value["schema"] != 1
        || value["command"] != command
        || value["status"] != status
        || value.get("data").is_none()
    {
        return Err("CLI exit or schema does not match the required outcome".into());
    }
    Ok(value)
}
pub(crate) fn gate_plan() -> Vec<Vec<&'static str>> {
    vec![
        vec!["fmt", "--all", "--check"],
        vec![
            "test",
            "--workspace",
            "--all-targets",
            "--locked",
            "-j",
            "2",
        ],
        vec![
            "test",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "-j",
            "2",
        ],
        vec![
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "-j",
            "2",
            "--",
            "-D",
            "warnings",
        ],
        vec![
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "-j",
            "2",
            "--",
            "-D",
            "warnings",
        ],
        vec!["doc", "--workspace", "--no-deps", "--locked", "-j", "2"],
        vec![
            "doc",
            "--workspace",
            "--no-deps",
            "--all-features",
            "--locked",
            "-j",
            "2",
        ],
    ]
}
fn benchmark_records(bytes: &[u8]) -> Result<Vec<Value>> {
    if bytes.len() > process::MAX_CAPTURE {
        return Err("benchmark output exceeds recording limit".into());
    }
    let records: Vec<Value> = std::str::from_utf8(bytes)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    let environment = records.first().ok_or("benchmark produced no records")?;
    if environment["record"] != "environment"
        || environment["schema"] != 1
        || environment["debug_assertions"] != false
        || environment
            .get("test_smoke")
            .is_some_and(|value| value != &Value::Bool(false))
        || !records
            .iter()
            .any(|record| record["record"] == "sample" && record["elapsed_ns"].as_u64().is_some())
    {
        return Err("benchmark did not produce a measured release sample".into());
    }
    Ok(records)
}
fn deny_version(bytes: &[u8]) -> Result<()> {
    if std::str::from_utf8(bytes)?.trim() != "cargo-deny 0.20.2" {
        return Err(
            "cargo-deny 0.20.2 is required; set ATTOW_CARGO_DENY to the verified tool".into(),
        );
    }
    Ok(())
}
fn native_host(bytes: &[u8]) -> Result<String> {
    let mut hosts = std::str::from_utf8(bytes)?
        .lines()
        .filter_map(|line| line.strip_prefix("host: "));
    let host = hosts.next().ok_or("missing native Rust host")?;
    if hosts.next().is_some()
        || host.len() > 128
        || host.split('-').count() < 3
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("invalid native Rust host".into());
    }
    Ok(host.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn license_gate_requires_the_verified_tool_version() {
        assert!(deny_version(b"cargo-deny 0.20.2\n").is_ok());
        assert!(deny_version(b"cargo-deny 0.19.8\n").is_err());
        assert!(deny_version(b"cargo-deny 0.20.2-unverified\n").is_err());
    }
    #[test]
    fn advisory_target_comes_from_the_actual_native_compiler_host() {
        assert_eq!(
            native_host(b"rustc 1.95.0\nhost: x86_64-pc-windows-msvc\nrelease: 1.95.0\n")
                .expect("host"),
            "x86_64-pc-windows-msvc"
        );
        assert!(native_host(b"host: --all\n").is_err());
        assert!(
            native_host(b"host: x86_64-pc-windows-msvc\nhost: aarch64-apple-darwin\n").is_err()
        );
        assert!(native_host(b"no host output\n").is_err());
    }
    #[test]
    fn parser_preserves_space_paths_and_rejects_malformed_or_unsafe_flags() {
        assert_eq!(
            parse(["demo", "--record-json", "a path/events.json"].map(OsString::from))
                .expect("demo"),
            Task::Demo(PathBuf::from("a path/events.json"))
        );
        assert_eq!(
            parse(["install-smoke", "--root", "install root"].map(OsString::from))
                .expect("install"),
            Task::InstallSmoke(PathBuf::from("install root"))
        );
        assert_eq!(
            parse(["release-check", "--local"].map(OsString::from)).expect("release"),
            Task::ReleaseCheck
        );
        assert_eq!(
            parse(["bench-report", "--output", "bench.json"].map(OsString::from)).expect("bench"),
            Task::BenchReport(PathBuf::from("bench.json"))
        );
        for args in [
            vec!["demo"],
            vec!["demo", "--record-json", "x", "--record-json", "y"],
            vec!["release-check"],
            vec!["release-check", "--force"],
            vec!["install-smoke", "--root", ""],
            vec!["verify-repository", "extra"],
        ] {
            assert!(parse(args.into_iter().map(OsString::from)).is_err());
        }
    }
    #[test]
    fn actual_envelopes_require_matching_exit_schema_command_and_status() {
        let success = serde_json::to_vec(
            &json!({"schema":1,"command":"run","status":"success","data":{"run_id":"r"}}),
        )
        .expect("JSON");
        assert!(envelope(&success, 0, 0, "run").is_ok());
        let unknown = serde_json::to_vec(
            &json!({"schema":1,"command":"run","status":"unknown","data":{"run_id":"r"}}),
        )
        .expect("JSON");
        assert!(envelope(&unknown, 3, 3, "run").is_ok());
        assert!(envelope(&unknown, 3, 0, "run").is_err());
        assert!(envelope(&success, 3, 3, "run").is_err());
        assert!(envelope(&success, 0, 0, "verify").is_err());
        assert!(envelope(b"not JSON", 0, 0, "run").is_err());
        assert!(envelope(b"{\"schema\":2}", 0, 0, "run").is_err());
    }
    #[test]
    fn release_plan_runs_independent_locked_gates_without_recursing() {
        let plan = gate_plan();
        assert!(plan.iter().any(|args| args == &["fmt", "--all", "--check"]));
        assert!(plan.iter().any(|args| args.contains(&"test")
            && args.contains(&"--all-features")
            && args.contains(&"--locked")));
        assert!(plan.iter().any(|args| args.contains(&"test")
            && !args.contains(&"--all-features")
            && args.contains(&"--locked")));
        assert!(
            plan.iter()
                .any(|args| args.contains(&"clippy") && args.contains(&"--all-features"))
        );
        assert!(plan.iter().any(|args| args.contains(&"doc")));
        assert!(!plan.iter().flatten().any(|arg| *arg == "release-check"));
    }
    #[test]
    fn benchmark_report_accepts_executed_samples_and_rejects_empty_or_smoke_claims() {
        let raw = b"{\"record\":\"environment\",\"schema\":1,\"debug_assertions\":false,\"test_smoke\":false}\n{\"record\":\"sample\",\"elapsed_ns\":10}\n";
        assert_eq!(benchmark_records(raw).expect("actual records").len(), 2);
        assert!(benchmark_records(b"").is_err());
        assert!(benchmark_records(b"{\"record\":\"environment\",\"schema\":1,\"debug_assertions\":true}\n{\"record\":\"sample\",\"elapsed_ns\":10}\n").is_err());
        assert!(benchmark_records(b"{\"record\":\"summary\"}\n").is_err());
    }
    #[test]
    fn subprocess_failure_is_observed_without_shell_interpretation() {
        let directory = tempfile::Builder::new()
            .prefix("Attow check with spaces ")
            .tempdir()
            .expect("tempdir");
        let output = process::capture(
            std::ffi::OsStr::new("rustc"),
            &process::args(&["--attow-deliberately-invalid-option"]),
            directory.path(),
            std::time::Duration::from_secs(20),
        )
        .expect("rustc ran");
        assert_ne!(output.code, 0);
        assert!(process::success(&output).is_err());
    }
}
