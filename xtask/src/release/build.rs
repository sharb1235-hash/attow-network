//! Native candidate producer. Verification itself never executes these payloads.
use super::{ReleaseError, Result, archive, manifest::*};
use crate::commands::process;
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn validate_gate_report(value: &Value, source: &SourceRevision, lock: &str) -> Result<()> {
    let checks = value["checks"]
        .as_array()
        .ok_or(ReleaseError::Invalid("missing local checks"))?;
    if value["schema"] != 1
        || value["status"] != "success"
        || source.dirty
        || value["source_before"] != serde_json::to_value(source)?
        || value["source_after"] != serde_json::to_value(source)?
        || value["lock_sha256_before"] != lock
        || value["lock_sha256_after"] != lock
        || checks.len() != 12
        || checks.iter().any(|c| c["exit_code"] != 0)
    {
        return Err(ReleaseError::Invalid(
            "release checks do not bind this clean source and lock",
        ));
    }
    for command in ["verify-repository", "demo", "install-smoke"] {
        if checks.iter().filter(|c| c["command"] == command).count() != 1 {
            return Err(ReleaseError::Invalid("missing release check"));
        }
    }
    for args in crate::commands::gate_plan() {
        if checks.iter().filter(|c| c["argv"] == json!(args)).count() != 1 {
            return Err(ReleaseError::Invalid("missing Cargo gate"));
        }
    }
    if checks
        .iter()
        .filter(|c| {
            c["argv"] == json!(["cargo-deny", "--locked", "--offline", "check", "licenses"])
                && c["version"] == "0.20.2"
        })
        .count()
        != 1
        || checks
            .iter()
            .filter(|c| {
                c["argv"].as_array().is_some_and(|a| {
                    a.len() == 7
                        && a[0] == "cargo-deny"
                        && a[1] == "--locked"
                        && a[2] == "--offline"
                        && a[3] == "--target"
                        && a[5] == "check"
                        && a[6] == "advisories"
                }) && c["version"] == "0.20.2"
            })
            .count()
            != 1
    {
        return Err(ReleaseError::Invalid("missing dependency policy gates"));
    }
    Ok(())
}
fn capture(root: &Path, program: &OsStr, args: &[&str], timeout: u64) -> Result<process::Output> {
    let out = process::capture(
        program,
        &process::args(args),
        root,
        Duration::from_secs(timeout),
    )
    .map_err(|e| ReleaseError::Tool(e.to_string()))?;
    if out.code != 0 {
        return Err(ReleaseError::Tool(format!(
            "{} failed with exit {}",
            program.to_string_lossy(),
            out.code
        )));
    }
    Ok(out)
}
fn compiler(root: &Path) -> Result<(String, String)> {
    let out = capture(
        root,
        OsStr::new("rustc"),
        &["+1.95.0", "--version", "--verbose"],
        30,
    )?;
    let text =
        String::from_utf8(out.stdout).map_err(|_| ReleaseError::Invalid("compiler output"))?;
    let target = text
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or(ReleaseError::Invalid("compiler host"))?
        .to_owned();
    if !text.lines().any(|l| l == "release: 1.95.0") {
        return Err(ReleaseError::Invalid("compiler version"));
    }
    Ok((text, target))
}
pub fn plan(root: &Path) -> Result<Value> {
    let (rustc, target) = compiler(root)?;
    Ok(
        json!({"schema":1,"project":"Attow Network","version":"0.1.0","native_target":target,"rustc_verbose":rustc,"source":source_revision(root)?,"candidate_target":"x86_64-pc-windows-msvc","build_profile":"release","archive_format":"bounded regular-file ustar","signing":"requires external maintainer configuration; integrity alone is not readiness","other_targets":{"linux":"blocked by unignored RUSTSEC-2024-0436 in Linux-only transitive dependency","macos":"not qualified"},"prerequisites":["clean committed source","matching successful release-check --local report","verified Node 24.21.0, Playwright 1.64.0, Chromium revision 1248"],"publishing":false}),
    )
}
fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    super::manifest::reject_link(source)?;
    if let Some(p) = destination.parent() {
        fs::create_dir_all(p)?;
    }
    use std::io::Write;
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    std::io::copy(&mut fs::File::open(source)?, &mut out)?;
    out.flush()?;
    Ok(())
}
fn files_under(dir: &Path, prefix: &str, files: &mut Vec<(String, PathBuf)>) -> Result<()> {
    super::manifest::reject_link(dir)?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        super::manifest::reject_link(&entry.path())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| ReleaseError::Invalid("artifact filename"))?;
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        safe_relative(&path)?;
        if entry.file_type()?.is_dir() {
            files_under(&entry.path(), &path, files)?;
        } else if entry.file_type()?.is_file() {
            files.push((path, entry.path()));
        } else {
            return Err(ReleaseError::Invalid("non-file payload"));
        }
        if files.len() > 8192 {
            return Err(ReleaseError::Invalid("too many payload files"));
        }
    }
    Ok(())
}
fn install(
    root: &Path,
    output: &Path,
    archive_path: &str,
    target: &str,
    source: &SourceRevision,
) -> Result<(tempfile::TempDir, PathBuf, String)> {
    let temporary = tempfile::Builder::new()
        .prefix("attow archive install ")
        .tempdir()?;
    let extracted = temporary.path().join("extracted payload");
    archive::extract_new(&output.join(archive_path), &extracted)?;
    let binary = extracted.join("bin/attow.exe");
    let binary_hash = hash_file(&binary)?.1;
    let cwd = temporary.path().join("offline workspace with spaces");
    fs::create_dir(&cwd)?;
    let mut checks = Vec::new();
    let mut run = |args: &[&str]| -> Result<Value> {
        let out = capture(&cwd, binary.as_os_str(), args, 300)?;
        let evidence = format!("evidence/archive-command-{}.txt", checks.len());
        fs::write(output.join(&evidence), &out.stdout)?;
        checks.push(Check {
            argv: args.iter().map(|s| (*s).to_owned()).collect(),
            exit_code: out.code,
            evidence,
        });
        if args == ["--help"] {
            if !String::from_utf8_lossy(&out.stdout).contains("Attow Network") {
                return Err(ReleaseError::Invalid("installed help"));
            }
            return Ok(Value::Null);
        }
        let value: Value = serde_json::from_slice(&out.stdout)?;
        if value["schema"] != 1 || value["status"] != "success" || value["command"] != args[0] {
            return Err(ReleaseError::Invalid("installed CLI envelope"));
        }
        Ok(value)
    };
    run(&["--help"])?;
    let doctor = run(&["doctor", "--json"])?;
    write_json(&output.join("runtime-profile.json"), &doctor)?;
    let profile_hash = doctor["data"]["runtime_profile_hash"]
        .as_str()
        .ok_or(ReleaseError::Invalid("doctor profile hash"))?
        .to_owned();
    if profile_hash.len() != 64 {
        return Err(ReleaseError::Invalid("runtime profile identity"));
    }
    run(&["init", ".", "--json"])?;
    let data = run(&["run", "data-change", "--workers", "2", "--json"])?;
    let id = data["data"]["run_id"]
        .as_str()
        .ok_or(ReleaseError::Invalid("installed run identity"))?;
    run(&["replay", id, "--offline", "--json"])?;
    run(&["verify", id, "--json"])?;
    write_json(
        &output.join("evidence/archive-install.json"),
        &InstallEvidence {
            schema: 1,
            target: target.into(),
            archive_path: archive_path.into(),
            archive_sha256: hash_file(&output.join(archive_path))?.1,
            binary_path: "bin/attow.exe".into(),
            binary_sha256: binary_hash,
            profile: "release".into(),
            source: source.clone(),
            checks,
        },
    )?;
    let _ = root;
    Ok((temporary, binary, profile_hash))
}
pub fn build(root: &Path, target: &str, output: &Path) -> Result<()> {
    let source = source_revision(root)?;
    if source.dirty {
        return Err(ReleaseError::Invalid(
            "commit source before candidate production",
        ));
    }
    let (rustc_verbose, host) = compiler(root)?;
    if target != host || target != "x86_64-pc-windows-msvc" {
        return Err(ReleaseError::Invalid(
            "only the qualified native Windows target can be built",
        ));
    }
    let lock = hash_file(&root.join("Cargo.lock"))?.1;
    let report: Value =
        serde_json::from_slice(&fs::read(root.join("target/release-check/report.json"))?)?;
    validate_gate_report(&report, &source, &lock)?;
    if !report["checks"]
        .as_array()
        .ok_or(ReleaseError::Invalid("checks"))?
        .iter()
        .any(|c| c["scope"] == "actual native compiler host only" && c["argv"][4] == host)
    {
        return Err(ReleaseError::Invalid("advisory gate target"));
    }
    let output = if output.is_absolute() {
        output.to_owned()
    } else {
        root.join(output)
    };
    fs::create_dir(&output)?;
    fs::create_dir(output.join("evidence"))?;
    eprintln!("Building native optimized Attow Network CLI.");
    let argv = [
        "build",
        "-p",
        "attow-cli",
        "--release",
        "--features",
        "browser",
        "--locked",
        "-j",
        "2",
    ];
    let build = process::cargo(root, &process::args(&argv))
        .map_err(|e| ReleaseError::Tool(e.to_string()))?;
    fs::write(output.join("evidence/native-build.log"), &build.stdout)?;
    if build.code != 0 {
        return Err(ReleaseError::Tool("native release build failed".into()));
    }
    let metadata = process::cargo(
        root,
        &process::args(&["metadata", "--format-version", "1", "--no-deps", "--locked"]),
    )
    .map_err(|e| ReleaseError::Tool(e.to_string()))?;
    if metadata.code != 0 {
        return Err(ReleaseError::Tool("Cargo metadata failed".into()));
    }
    let metadata: Value = serde_json::from_slice(&metadata.stdout)?;
    let target_dir = PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .ok_or(ReleaseError::Invalid("Cargo target directory"))?,
    );
    let mut platform_files = vec![("bin/attow.exe".into(), target_dir.join("release/attow.exe"))];
    for name in [
        "README.md",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "THIRD_PARTY_NOTICES.md",
        "docs/getting-started.md",
        "docs/release-runbook.md",
        "release-toolchain.json",
    ] {
        platform_files.push((name.into(), root.join(name)));
    }
    let source_files = capture(
        root,
        OsStr::new("git"),
        &[
            "ls-files",
            "-z",
            "demos/workflow-studio/browser",
            "demos/workflow-studio/app",
            "docs/notices/stage7",
            "docs/notices/stage8",
        ],
        30,
    )?;
    for name in source_files
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
    {
        let name = std::str::from_utf8(name).map_err(|_| ReleaseError::Invalid("tracked path"))?;
        if archive::allowed_source(name) {
            platform_files.push((name.into(), root.join(name)));
        }
    }
    let inventory = super::inventory::generate(root, target, &output)?;
    for path in &inventory.artifacts {
        if path.starts_with("notices/") {
            platform_files.push((format!("third-party/{path}"), artifact_path(&output, path)?));
        }
    }
    let archive_path = format!("attow-network-0.1.0-{target}.tar");
    archive::write_files(&output.join(&archive_path), &platform_files)?;
    archive::source_archive(root, &output.join("attow-network-0.1.0-source.tar"))?;
    let (installed, binary, runtime_profile_hash) =
        install(root, &output, &archive_path, target, &source)?;
    let mut runtime = super::demo_record::RuntimeOverrides::from_env()?;
    runtime.sidecar = binary
        .parent()
        .and_then(Path::parent)
        .ok_or(ReleaseError::Invalid("archive layout"))?
        .join("demos/workflow-studio/browser/sidecar.mjs");
    let browser_dir = runtime
        .sidecar
        .parent()
        .ok_or(ReleaseError::Invalid("browser layout"))?;
    let npm = runtime
        .node
        .parent()
        .ok_or(ReleaseError::Invalid("Node layout"))?
        .join("node_modules/npm/bin/npm-cli.js");
    let provision = process::capture(
        runtime.node.as_os_str(),
        &[
            npm.into_os_string(),
            "ci".into(),
            "--ignore-scripts".into(),
            "--no-audit".into(),
            "--no-fund".into(),
        ],
        browser_dir,
        Duration::from_secs(300),
    )
    .map_err(|e| ReleaseError::Tool(e.to_string()))?;
    fs::write(
        output.join("evidence/archive-browser-provision.log"),
        &provision.stdout,
    )?;
    if provision.code != 0 {
        return Err(ReleaseError::Tool(
            "archive browser dependency provision failed".into(),
        ));
    }
    super::demo_record::record_with_binary(
        root,
        &output.join("demo"),
        &binary,
        "release",
        &runtime,
    )?;
    drop(installed);
    let mut build_checks = vec![Check {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        exit_code: 0,
        evidence: "evidence/native-build.log".into(),
    }];
    copy_file(
        &root.join("target/release-check/report.json"),
        &output.join("evidence/release-check/report.json"),
    )?;
    for check in report["checks"]
        .as_array()
        .ok_or(ReleaseError::Invalid("checks"))?
    {
        let evidence = if let Some(path) = check["stdout"]
            .as_str()
            .or_else(|| check["recording"].as_str())
        {
            safe_relative(path)?;
            let relative = format!("evidence/release-check/{path}");
            copy_file(
                &root.join("target/release-check").join(path),
                &output.join(&relative),
            )?;
            relative
        } else {
            "evidence/release-check/report.json".into()
        };
        let argv = if let Some(args) = check["argv"].as_array() {
            args.iter()
                .map(|v| v.as_str().unwrap_or("").to_owned())
                .collect()
        } else {
            vec![
                check["command"]
                    .as_str()
                    .ok_or(ReleaseError::Invalid("gate command"))?
                    .to_owned(),
            ]
        };
        build_checks.push(Check {
            argv,
            exit_code: 0,
            evidence,
        });
    }
    if source_revision(root)? != source || hash_file(&root.join("Cargo.lock"))?.1 != lock {
        return Err(ReleaseError::Invalid(
            "source changed during candidate production",
        ));
    }
    write_json(
        &output.join("provenance.json"),
        &json!({"schema":1,"source":source,"target":target,"rustc_verbose":rustc_verbose,"lock_sha256":lock,"runtime_profile_hash":runtime_profile_hash,"build_profile":"release","build_checks":build_checks,"independent_reproducibility":"not_measured","archive_install":"fresh directory on current Windows host","external_runtime":"Node and Chromium explicitly provisioned; not bundled","source_install_profile":"debug; actual fresh prefix from matching release-check"}),
    )?;
    let mut files = Vec::new();
    files_under(&output, "", &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut artifacts = Vec::new();
    for (path, file) in files {
        let role = if path == archive_path {
            "platform-archive"
        } else if path.ends_with("-source.tar") {
            "source-archive"
        } else if path == "sbom.cdx.json" {
            "sbom"
        } else if path == "dependency-inventory.json" {
            "dependency-inventory"
        } else if path.starts_with("notices/") {
            "third-party-notices"
        } else if path == "runtime-profile.json" {
            "runtime-profile"
        } else if path == "provenance.json" {
            "provenance"
        } else if path.starts_with("demo/") {
            "demo"
        } else {
            "evidence"
        };
        let (bytes, sha256) = hash_file(&file)?;
        artifacts.push(Artifact {
            path,
            bytes,
            sha256,
            role: role.into(),
        });
    }
    let checksums = artifacts
        .iter()
        .map(|a| format!("{}  {}\n", a.sha256, a.path))
        .collect::<String>();
    fs::write(output.join("SHA256SUMS"), checksums)?;
    let manifest=ReleaseManifest{schema:1,project:"Attow Network".into(),version:"0.1.0".into(),source,target:target.into(),rustc_verbose,lock_sha256:lock,runtime_profile_hash,artifacts,build_checks,expected_signing_identity:super::sign::configured_identity(&output)?,supported_features:vec![Feature{name:"local WASM/SQL/peer workflow and offline replay".into(),status:"passed".into(),prerequisites:vec!["owned same-host fence; simulated provider".into()]},Feature{name:"owned browser fixture".into(),status:"passed".into(),prerequisites:vec!["Node 24.21.0, Playwright 1.64.0, Chromium revision 1248; configured absolute runtime paths".into()]}],supported_targets:vec![target.into()],install_evidence:"evidence/archive-install.json".into(),demo_evidence:"demo/recording.json".into()};
    write_json(&output.join("manifest.json"), &manifest)?;
    let verified =
        super::verify::verify_candidate(&output, super::verify::VerificationMode::IntegrityOnly)?;
    println!("{}", serde_json::to_string_pretty(&verified)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_checks_must_bind_clean_unchanged_source_and_lock() {
        let source = SourceRevision {
            revision: "a".repeat(40),
            dirty: false,
        };
        let lock = "b".repeat(64);
        for report in [
            serde_json::json!({"schema":1,"status":"success","checks":[]}),
            serde_json::json!({"schema":1,"status":"success","source_before":source,"source_after":source,"lock_sha256_before":lock,"lock_sha256_after":lock,"checks":[{"command":"test","exit_code":0}]}),
        ] {
            assert!(validate_gate_report(&report, &source, &lock).is_err());
        }
    }
    #[test]
    fn complete_gate_report_rejects_stale_source_changed_lock_and_missing_feature_lane()
    -> Result<()> {
        let source = SourceRevision {
            revision: "a".repeat(40),
            dirty: false,
        };
        let lock = "b".repeat(64);
        let mut checks: Vec<Value> = crate::commands::gate_plan()
            .into_iter()
            .map(|argv| json!({"argv":argv,"exit_code":0}))
            .collect();
        for command in ["verify-repository", "demo", "install-smoke"] {
            checks.push(json!({"command":command,"exit_code":0}));
        }
        checks.push(json!({"argv":["cargo-deny","--locked","--offline","check","licenses"],"version":"0.20.2","exit_code":0}));
        checks.push(json!({"argv":["cargo-deny","--locked","--offline","--target","x86_64-pc-windows-msvc","check","advisories"],"version":"0.20.2","exit_code":0}));
        let report = json!({"schema":1,"status":"success","source_before":source,"source_after":source,"lock_sha256_before":lock,"lock_sha256_after":lock,"checks":checks});
        validate_gate_report(&report, &source, &lock)?;
        let mut changed = report.clone();
        changed["source_after"]["dirty"] = json!(true);
        assert!(validate_gate_report(&changed, &source, &lock).is_err());
        let mut changed = report.clone();
        changed["lock_sha256_after"] = json!("c".repeat(64));
        assert!(validate_gate_report(&changed, &source, &lock).is_err());
        let mut changed = report;
        changed["checks"][2] = json!({"argv":["test","--workspace"],"exit_code":0});
        assert!(validate_gate_report(&changed, &source, &lock).is_err());
        Ok(())
    }
}
