use super::{
    Result, benchmark_records, deny_version, envelope, gate_plan, native_host,
    process::{self, args},
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

fn destination(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}
fn save(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or("recording output needs a parent directory")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(&mut file)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
fn target_dir(root: &Path) -> Result<PathBuf> {
    let output = process::cargo(
        root,
        &args(&["metadata", "--format-version", "1", "--no-deps", "--locked"]),
    )?;
    process::success(&output)?;
    let value: Value = serde_json::from_slice(&output.stdout)?;
    Ok(PathBuf::from(
        value["target_directory"]
            .as_str()
            .ok_or("Cargo did not identify its target directory")?,
    ))
}
fn binary(prefix: &Path) -> PathBuf {
    prefix.join(if cfg!(windows) { "attow.exe" } else { "attow" })
}
fn cli(
    binary: &Path,
    cwd: &Path,
    arguments: &[&str],
    expected: i32,
    records: &mut Vec<Value>,
) -> Result<Value> {
    let output = process::capture(
        binary.as_os_str(),
        &args(arguments),
        cwd,
        Duration::from_secs(300),
    )?;
    let envelope = envelope(
        &output.stdout,
        output.code,
        expected,
        arguments.first().ok_or("missing CLI command")?,
    )?;
    records.push(json!({"argv":arguments,"exit_code":output.code,"envelope":envelope}));
    Ok(envelope["data"].clone())
}
fn run_id(data: &Value) -> Result<String> {
    let id = data["run_id"]
        .as_str()
        .ok_or("run output did not contain a run identifier")?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("invalid run identifier in CLI output".into());
    }
    if !data["events"].is_array()
        || !data["summary"].is_object()
        || data["bundle_path"].as_str().is_none_or(|path| {
            Path::new(path).is_absolute() || path.split(['/', '\\']).any(|part| part == "..")
        })
    {
        return Err("run output did not contain a portable event record".into());
    }
    Ok(id.to_owned())
}
fn workspace(parent: &Path, label: &str) -> Result<PathBuf> {
    fs::create_dir_all(parent)?;
    Ok(tempfile::Builder::new()
        .prefix(label)
        .tempdir_in(parent)?
        .keep())
}
pub(super) fn demo(root: &Path, output_path: &Path) -> Result<()> {
    let output_path = destination(root, output_path);
    let build = process::cargo(
        root,
        &args(&[
            "build",
            "-p",
            "attow-cli",
            "--features",
            "browser",
            "--locked",
            "-j",
            "2",
        ]),
    )?;
    process::success(&build)?;
    let binary = binary(&target_dir(root)?.join("debug"));
    let cwd = workspace(
        output_path.parent().ok_or("recording directory")?,
        "owned demo ",
    )?;
    let mut commands = Vec::new();
    cli(&binary, &cwd, &["init", ".", "--json"], 0, &mut commands)?;
    let paused = cli(
        &binary,
        &cwd,
        &[
            "run",
            "checkout",
            "--workers",
            "2",
            "--browser",
            "--pause-after-unknown",
            "--json",
        ],
        3,
        &mut commands,
    )?;
    let id = run_id(&paused)?;
    let checkout = cli(
        &binary,
        &cwd,
        &[
            "run",
            "checkout",
            "--workers",
            "2",
            "--browser",
            "--resume",
            &id,
            "--json",
        ],
        0,
        &mut commands,
    )?;
    if run_id(&checkout)? != id {
        return Err("resume changed the logical run identity".into());
    }
    cli(
        &binary,
        &cwd,
        &["replay", &id, "--offline", "--json"],
        0,
        &mut commands,
    )?;
    cli(&binary, &cwd, &["verify", &id, "--json"], 0, &mut commands)?;
    let data_change = cli(
        &binary,
        &cwd,
        &["run", "data-change", "--workers", "2", "--json"],
        0,
        &mut commands,
    )?;
    let data_id = run_id(&data_change)?;
    cli(
        &binary,
        &cwd,
        &["replay", &data_id, "--offline", "--json"],
        0,
        &mut commands,
    )?;
    cli(
        &binary,
        &cwd,
        &["verify", &data_id, "--json"],
        0,
        &mut commands,
    )?;
    save(
        &output_path,
        &json!({"schema":1,"fixture_mode":"owned","workspace":cwd.file_name().and_then(|name|name.to_str()),"commands":commands,"runs":[checkout,data_change]}),
    )?;
    println!("Attow Network owned demo recorded actual CLI output.");
    Ok(())
}
pub(super) fn install_smoke(root: &Path, install_root: &Path) -> Result<()> {
    let install_root = destination(root, install_root);
    let attempt = workspace(&install_root, "source install ")?;
    let prefix = attempt.join("install prefix");
    let cwd = attempt.join("offline workspace with spaces");
    fs::create_dir_all(&cwd)?;
    let mut arguments = args(&["install", "--path"]);
    arguments.push(root.join("crates/chronos-cli").into_os_string());
    arguments.extend(args(&["--locked", "--debug", "--root"]));
    arguments.push(prefix.clone().into_os_string());
    arguments.push(OsString::from("--target-dir"));
    arguments.push(target_dir(root)?.into_os_string());
    arguments.extend(args(&["-j", "2"]));
    let install = process::cargo(root, &arguments)?;
    process::success(&install)?;
    let binary = binary(&prefix.join("bin"));
    let mut commands = Vec::new();
    cli(&binary, &cwd, &["init", ".", "--json"], 0, &mut commands)?;
    cli(&binary, &cwd, &["doctor", "--json"], 0, &mut commands)?;
    let run = cli(
        &binary,
        &cwd,
        &["run", "data-change", "--workers", "2", "--json"],
        0,
        &mut commands,
    )?;
    let id = run_id(&run)?;
    cli(
        &binary,
        &cwd,
        &["replay", &id, "--offline", "--json"],
        0,
        &mut commands,
    )?;
    cli(&binary, &cwd, &["verify", &id, "--json"], 0, &mut commands)?;
    save(
        &install_root.join("report.json"),
        &json!({"schema":1,"fixture_mode":"owned","install_profile":"debug","fresh_prefix":true,"attempt":attempt.file_name().and_then(|name|name.to_str()),"commands":commands,"runs":[run]}),
    )?;
    println!("Attow Network source install, offline replay and verification passed.");
    Ok(())
}
pub(super) fn release_check(root: &Path) -> Result<()> {
    let source_before = crate::release::manifest::source_revision(root)?;
    let lock_sha256_before = crate::release::manifest::hash_file(&root.join("Cargo.lock"))?.1;
    let directory = root.join("target/release-check");
    fs::create_dir_all(&directory)?;
    let report = directory.join("report.json");
    let mut checks = Vec::new();
    let result: Result<()> = (|| {
        crate::verify_repository(root)?;
        checks.push(json!({"command":"verify-repository","exit_code":0}));
        let deny =
            std::env::var_os("ATTOW_CARGO_DENY").unwrap_or_else(|| OsString::from("cargo-deny"));
        let version =
            process::capture(&deny, &args(&["--version"]), root, Duration::from_secs(30))?;
        process::success(&version)?;
        deny_version(&version.stdout)?;
        let license = process::capture(
            &deny,
            &args(&["--locked", "--offline", "check", "licenses"]),
            root,
            Duration::from_secs(300),
        )?;
        fs::write(directory.join("licenses.log"), &license.stdout)?;
        checks.push(json!({"argv":["cargo-deny","--locked","--offline","check","licenses"],"version":"0.20.2","exit_code":license.code,"elapsed_ms":license.elapsed_ms,"stdout":"licenses.log"}));
        process::success(&license)?;
        let compiler = process::capture(
            std::ffi::OsStr::new("rustc"),
            &args(&["+1.95.0", "--version", "--verbose"]),
            root,
            Duration::from_secs(30),
        )?;
        process::success(&compiler)?;
        let host = native_host(&compiler.stdout)?;
        let advisory_args = args(&[
            "--locked",
            "--offline",
            "--target",
            &host,
            "check",
            "advisories",
        ]);
        let advisories = process::capture(&deny, &advisory_args, root, Duration::from_secs(300))?;
        fs::write(directory.join("native-advisories.log"), &advisories.stdout)?;
        checks.push(json!({"argv":["cargo-deny","--locked","--offline","--target",host,"check","advisories"],"version":"0.20.2","scope":"actual native compiler host only","exit_code":advisories.code,"elapsed_ms":advisories.elapsed_ms,"stdout":"native-advisories.log"}));
        process::success(&advisories)?;
        for (index, arguments) in gate_plan().into_iter().enumerate() {
            eprintln!("Attow Network release check: cargo {}", arguments.join(" "));
            let output = process::cargo(root, &args(&arguments))?;
            let log = format!("gate-{index}.log");
            fs::write(directory.join(&log), &output.stdout)?;
            checks.push(json!({"argv":arguments,"exit_code":output.code,"elapsed_ms":output.elapsed_ms,"stdout":log}));
            save(
                &report,
                &json!({"schema":1,"status":"running","checks":checks}),
            )?;
            process::success(&output)?;
        }
        demo(root, &directory.join("demo.json"))?;
        checks.push(json!({"command":"demo","exit_code":0,"recording":"demo.json"}));
        install_smoke(root, &directory.join("install smoke"))?;
        checks.push(json!({"command":"install-smoke","exit_code":0,"recording":"install smoke/report.json"}));
        Ok(())
    })();
    save(
        &report,
        &json!({"schema":1,"status":if result.is_ok(){"success"}else{"failed"},"checks":checks,
            "source_before":source_before,"source_after":crate::release::manifest::source_revision(root)?,
            "lock_sha256_before":lock_sha256_before,"lock_sha256_after":crate::release::manifest::hash_file(&root.join("Cargo.lock"))?.1}),
    )?;
    result?;
    println!(
        "Attow Network local release checks passed; actual evidence is in target/release-check."
    );
    Ok(())
}
pub(super) fn bench_report(root: &Path, output_path: &Path) -> Result<()> {
    let mut harnesses = Vec::new();
    for (name, features) in [("worldline", "local"), ("scheduler", "local,scheduler")] {
        let arguments = args(&[
            "bench",
            "--bench",
            name,
            "--features",
            features,
            "--locked",
            "-j",
            "2",
        ]);
        let output = process::cargo(root, &arguments)?;
        process::success(&output)?;
        let records = benchmark_records(&output.stdout)?;
        harnesses.push(json!({"harness":name,"profile":"release","records":records}));
    }
    save(
        &destination(root, output_path),
        &json!({"schema":1,"fixture_mode":"owned","measurements":"executed local samples; not latency guarantees","harnesses":harnesses}),
    )?;
    println!("Attow Network executed benchmark records saved.");
    Ok(())
}
