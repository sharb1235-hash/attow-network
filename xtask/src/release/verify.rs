//! Offline artifact consistency checks. This module never executes a candidate.
use super::{
    ReleaseError, Result, archive,
    manifest::{self, Artifact, InstallEvidence, ReleaseManifest},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};

const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
const MAX_JSON: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationMode {
    IntegrityOnly,
    RequireSignatures,
}
#[derive(Clone, Debug, Serialize)]
pub struct VerificationReport {
    pub schema: u32,
    pub target: String,
    pub artifacts: usize,
    pub bytes: u64,
    pub manifest_sha256: String,
    pub signature_status: String,
    pub release_ready: bool,
}
pub fn verify_candidate(dir: &Path, mode: VerificationMode) -> Result<VerificationReport> {
    let (manifest, raw) = manifest::load(dir)?;
    validate_manifest(&manifest)?;
    let mut total = 0u64;
    for artifact in &manifest.artifacts {
        let path = manifest::artifact_path(dir, &artifact.path)?;
        if fs::metadata(&path)?.len() != artifact.bytes
            || manifest::hash_file(&path)? != (artifact.bytes, artifact.sha256.clone())
        {
            return Err(invalid("artifact bytes do not match the frozen manifest"));
        }
        total = total
            .checked_add(artifact.bytes)
            .ok_or(invalid("artifact aggregate overflow"))?;
    }
    checksums(
        &manifest.artifacts,
        &read(dir, "SHA256SUMS", 2 * 1024 * 1024)?,
    )?;
    let expected: BTreeSet<_> = manifest
        .artifacts
        .iter()
        .map(|a| a.path.clone())
        .chain(["manifest.json".into(), "SHA256SUMS".into()])
        .collect();
    let mut actual = BTreeSet::new();
    inventory(dir, dir, &mut actual, 0)?;
    let signature_present = actual.remove(super::sign::BUNDLE_NAME);
    if actual != expected {
        return Err(invalid("candidate contains unlisted or missing files"));
    }
    let platform = role(&manifest, "platform-archive")?;
    let platform_entries = archive::validate_tar(&manifest::artifact_path(dir, &platform.path)?)?;
    let source = role(&manifest, "source-archive")?;
    let source_entries = archive::validate_tar(&manifest::artifact_path(dir, &source.path)?)?;
    if source_entries
        .iter()
        .any(|e| !archive::allowed_source(&e.path))
        || !source_entries
            .iter()
            .any(|e| e.path == "Cargo.lock" && e.sha256 == manifest.lock_sha256)
        || !source_entries.iter().any(|e| e.path == "LICENSE-MIT")
        || !source_entries.iter().any(|e| e.path == "LICENSE-APACHE")
    {
        return Err(invalid(
            "source archive contains excluded material or lacks bound source files",
        ));
    }
    let install = verify_install(dir, &manifest, platform, &platform_entries)?;
    verify_build_evidence(dir, &manifest)?;
    verify_demo(dir, &manifest, &install, &platform_entries)?;
    let runtime: Value = json(dir, &role(&manifest, "runtime-profile")?.path)?;
    if runtime["schema"] != 1
        || runtime["command"] != "doctor"
        || runtime["status"] != "success"
        || runtime["data"]["runtime_profile_hash"] != manifest.runtime_profile_hash
    {
        return Err(invalid(
            "runtime profile evidence does not match the manifest",
        ));
    }
    let provenance: Value = json(dir, &role(&manifest, "provenance")?.path)?;
    if provenance["schema"] != 1
        || provenance["source"] != serde_json::to_value(&manifest.source)?
        || provenance["target"] != manifest.target
        || provenance["lock_sha256"] != manifest.lock_sha256
        || provenance["runtime_profile_hash"] != manifest.runtime_profile_hash
        || provenance["rustc_verbose"] != manifest.rustc_verbose
        || provenance["build_profile"] != "release"
        || provenance["build_checks"] != serde_json::to_value(&manifest.build_checks)?
    {
        return Err(invalid("provenance does not bind the same candidate"));
    }
    let signature_status = match mode {
        VerificationMode::IntegrityOnly => {
            if signature_present {
                "present_unverified"
            } else {
                "absent"
            }
        }
        VerificationMode::RequireSignatures => {
            if !signature_present {
                return Err(invalid(
                    "authentic signatures are required; candidate is unsigned",
                ));
            }
            super::sign::verify_manifest_signature(dir, &manifest, &raw)?;
            "verified"
        }
    };
    if manifest::load(dir)?.1 != raw {
        return Err(invalid("manifest changed during verification"));
    }
    Ok(VerificationReport {
        schema: 1,
        target: manifest.target,
        artifacts: manifest.artifacts.len(),
        bytes: total,
        manifest_sha256: manifest::hash_bytes(&raw),
        signature_status: signature_status.into(),
        release_ready: signature_status == "verified",
    })
}
fn invalid(message: &'static str) -> ReleaseError {
    ReleaseError::Invalid(message)
}
fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn checksums(artifacts: &[Artifact], bytes: &[u8]) -> Result<()> {
    let expected: String = artifacts
        .iter()
        .map(|a| format!("{}  {}\n", a.sha256, a.path))
        .collect();
    if bytes != expected.as_bytes() {
        return Err(invalid("SHA256SUMS is not the exact derived payload list"));
    }
    Ok(())
}
fn validate_manifest(manifest: &ReleaseManifest) -> Result<()> {
    if manifest.schema != 1
        || manifest.project != "Attow Network"
        || manifest.version != "0.1.0"
        || manifest.source.dirty
        || !lower_hex(&manifest.source.revision, 40)
        || !lower_hex(&manifest.lock_sha256, 64)
        || !lower_hex(&manifest.runtime_profile_hash, 64)
        || manifest.target != "x86_64-pc-windows-msvc"
        || manifest.supported_targets != [manifest.target.clone()]
        || !manifest
            .rustc_verbose
            .lines()
            .any(|line| line == "host: x86_64-pc-windows-msvc")
        || !manifest
            .rustc_verbose
            .lines()
            .any(|line| line == "release: 1.95.0")
        || manifest.artifacts.is_empty()
        || manifest.artifacts.len() > 8192
        || manifest.build_checks.len() != 13
        || manifest.supported_features.is_empty()
        || manifest.supported_features.len() > 64
    {
        return Err(invalid("unsupported or incomplete release manifest"));
    }
    let mut names = BTreeSet::new();
    let mut last = None;
    let mut bytes = 0u64;
    for artifact in &manifest.artifacts {
        manifest::safe_relative(&artifact.path)?;
        let lower = artifact.path.to_ascii_lowercase();
        if last.is_some_and(|v: &str| v >= artifact.path.as_str())
            || !names.insert(lower)
            || !lower_hex(&artifact.sha256, 64)
            || artifact.bytes > MAX_FILE
            || ["manifest.json", "SHA256SUMS", super::sign::BUNDLE_NAME]
                .contains(&artifact.path.as_str())
            || artifact.role.is_empty()
            || artifact.role.len() > 64
        {
            return Err(invalid(
                "artifact inventory is not a bounded sorted payload set",
            ));
        }
        bytes = bytes
            .checked_add(artifact.bytes)
            .ok_or(invalid("artifact aggregate overflow"))?;
        last = Some(&artifact.path);
    }
    if bytes > MAX_TOTAL {
        return Err(invalid("candidate aggregate exceeds limit"));
    }
    let mut features = BTreeSet::new();
    for feature in &manifest.supported_features {
        if feature.name.is_empty()
            || feature.name.len() > 256
            || !features.insert(&feature.name)
            || feature.status != "passed"
            || feature.prerequisites.len() > 32
            || feature.prerequisites.iter().any(|p| p.len() > 4096)
        {
            return Err(invalid("supported feature lacks a passing result"));
        }
    }
    for required in [
        "platform-archive",
        "source-archive",
        "sbom",
        "dependency-inventory",
        "runtime-profile",
        "provenance",
    ] {
        role(manifest, required)?;
    }
    if !manifest
        .artifacts
        .iter()
        .any(|a| a.role == "third-party-notices")
    {
        return Err(invalid("distribution notices are absent"));
    }
    for path in [&manifest.install_evidence, &manifest.demo_evidence] {
        listed(manifest, path)?;
    }
    for check in &manifest.build_checks {
        if check.exit_code != 0
            || check.argv.is_empty()
            || check.argv.len() > 32
            || check.argv.iter().any(|a| a.len() > 4096)
        {
            return Err(invalid("build check failed or exceeds limit"));
        }
        listed(manifest, &check.evidence)?;
    }
    Ok(())
}
fn role<'a>(manifest: &'a ReleaseManifest, role: &str) -> Result<&'a Artifact> {
    let mut entries = manifest.artifacts.iter().filter(|a| a.role == role);
    let entry = entries
        .next()
        .ok_or(invalid("required artifact role is absent"))?;
    if entries.next().is_some() {
        return Err(invalid("required singleton artifact role is duplicated"));
    }
    Ok(entry)
}
fn listed<'a>(manifest: &'a ReleaseManifest, path: &str) -> Result<&'a Artifact> {
    manifest::safe_relative(path)?;
    manifest
        .artifacts
        .iter()
        .find(|a| a.path == path)
        .ok_or(invalid("evidence is not in the artifact inventory"))
}
fn read(dir: &Path, path: &str, max: usize) -> Result<Vec<u8>> {
    let file = fs::File::open(manifest::artifact_path(dir, path)?)?;
    if file.metadata()?.len() > max as u64 {
        return Err(invalid("evidence file exceeds limit"));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(invalid("evidence file exceeds limit"));
    }
    Ok(bytes)
}
fn json<T: DeserializeOwned>(dir: &Path, path: &str) -> Result<T> {
    Ok(serde_json::from_slice(&read(dir, path, MAX_JSON)?)?)
}
fn inventory(root: &Path, dir: &Path, files: &mut BTreeSet<String>, depth: usize) -> Result<()> {
    inventory_with_limit(root, dir, files, depth, 16384)
}
fn inventory_with_limit(root: &Path, dir: &Path, files: &mut BTreeSet<String>, depth: usize, _max_entries: usize) -> Result<()> {
    if depth > 32 {
        return Err(invalid("candidate directory depth"));
    }
    manifest::reject_link(dir)?;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        manifest::reject_link(&path)?;
        if fs::metadata(&path)?.is_dir() {
            inventory(root, &path, files, depth + 1)?;
        } else if fs::metadata(&path)?.is_file() {
            let name = path
                .strip_prefix(root)
                .map_err(|_| invalid("candidate path"))?
                .to_str()
                .ok_or(invalid("non UTF-8 path"))?
                .replace('\\', "/");
            manifest::safe_relative(&name)?;
            files.insert(name);
            if files.len() > 8195 {
                return Err(invalid("candidate file limit"));
            }
        } else {
            return Err(invalid("candidate contains a non-regular file"));
        }
    }
    Ok(())
}
fn verify_install(
    dir: &Path,
    manifest: &ReleaseManifest,
    platform: &Artifact,
    entries: &[archive::ArchiveEntry],
) -> Result<InstallEvidence> {
    let install: InstallEvidence = json(dir, &manifest.install_evidence)?;
    if install.schema != 1
        || install.target != manifest.target
        || install.source != manifest.source
        || install.profile != "release"
        || install.archive_path != platform.path
        || install.archive_sha256 != platform.sha256
        || install.binary_path != "bin/attow.exe"
        || install.checks.len() != 6
    {
        return Err(invalid(
            "archive installation evidence is not bound to this release",
        ));
    }
    if !entries
        .iter()
        .any(|e| e.path == install.binary_path && e.sha256 == install.binary_sha256 && e.bytes > 0)
    {
        return Err(invalid(
            "installed executable differs from the archived executable",
        ));
    }
    let mut run_id = None;
    for (index, check) in install.checks.iter().enumerate() {
        let expected = ["--help", "doctor", "init", "run", "replay", "verify"][index];
        if check.exit_code != 0 || check.argv.first().map(String::as_str) != Some(expected) {
            return Err(invalid("archive installation command failed or is missing"));
        }
        listed(manifest, &check.evidence)?;
        let bytes = read(dir, &check.evidence, MAX_JSON)?;
        if index == 0 {
            if !String::from_utf8_lossy(&bytes).contains("Attow Network") {
                return Err(invalid("installed help evidence"));
            }
            continue;
        }
        let envelope: Value = serde_json::from_slice(&bytes)?;
        if envelope["schema"] != 1
            || envelope["command"] != expected
            || envelope["status"] != "success"
            || envelope.get("error").is_some()
        {
            return Err(invalid("installed CLI envelope is inconsistent"));
        }
        if expected == "run" {
            if check.argv.get(1).map(String::as_str) != Some("data-change")
                || envelope["data"]["phase"] != "completed"
            {
                return Err(invalid("archive install lacks the offline scenario"));
            }
            run_id = Some(text(&envelope["data"]["run_id"])?.to_owned());
        }
        if ["replay", "verify"].contains(&expected)
            && (check.argv.get(1).map(String::as_str) != run_id.as_deref()
                || envelope["data"]["run_id"].as_str() != run_id.as_deref()
                || envelope["data"]["worker_calls"] != 0
                || envelope["data"]["dispatch_calls"] != 0
                || (expected == "replay" && !check.argv.iter().any(|a| a == "--offline")))
        {
            return Err(invalid(
                "archive replay or verification is not bound to its executed run",
            ));
        }
    }
    Ok(install)
}
fn verify_build_evidence(dir: &Path, manifest: &ReleaseManifest) -> Result<()> {
    let report_path = "evidence/release-check/report.json";
    listed(manifest, report_path)?;
    let report: Value = json(dir, report_path)?;
    super::build::validate_gate_report(&report, &manifest.source, &manifest.lock_sha256)?;
    let checks = report["checks"]
        .as_array()
        .ok_or(invalid("missing checks"))?;
    if !checks.iter().any(|c| {
        c["scope"] == "actual native compiler host only" && c["argv"][4] == manifest.target
    }) {
        return Err(invalid(
            "native advisory evidence is for a different platform",
        ));
    }
    let native = vec![
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
    if manifest.build_checks[0].argv != native {
        return Err(invalid("native optimized build evidence is missing"));
    }
    for (record, check) in checks.iter().zip(&manifest.build_checks[1..]) {
        let argv = if let Some(a) = record["argv"].as_array() {
            a.iter()
                .map(|v| text(v).map(str::to_owned))
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![text(&record["command"])?.into()]
        };
        let evidence = record["stdout"]
            .as_str()
            .or_else(|| record["recording"].as_str())
            .map(|p| format!("evidence/release-check/{p}"))
            .unwrap_or(report_path.into());
        if check.argv != argv || check.evidence != evidence {
            return Err(invalid("build checks diverge from recorded release checks"));
        }
    }
    Ok(())
}
fn text(value: &Value) -> Result<&str> {
    value.as_str().ok_or(invalid("invalid evidence text"))
}
fn number(value: &Value) -> Result<u64> {
    value.as_u64().ok_or(invalid("invalid evidence number"))
}
fn verify_demo(
    dir: &Path,
    manifest: &ReleaseManifest,
    install: &InstallEvidence,
    entries: &[archive::ArchiveEntry],
) -> Result<()> {
    let record: Value = json(dir, &manifest.demo_evidence)?;
    let binary = entries
        .iter()
        .find(|e| e.path == install.binary_path)
        .ok_or(invalid("missing binary"))?;
    if record["schema"] != 1
        || record["source"] != serde_json::to_value(&manifest.source)?
        || record["fixture_mode"] != "owned"
        || record["provider"] != "simulated"
        || record["executable"]["profile"] != "release"
        || record["executable"]["sha256"] != binary.sha256
        || record["executable"]["bytes"] != binary.bytes
        || manifest.demo_evidence != "demo/recording.json"
    {
        return Err(invalid("demo is not bound to the installed archive binary"));
    }
    let files = record["files"]
        .as_array()
        .ok_or(invalid("demo file inventory"))?;
    if files.len() != 3 {
        return Err(invalid("demo recording lacks its replayable files"));
    }
    for (file, name) in files
        .iter()
        .zip(["events.json", "index.html", "terminal.cast"])
    {
        let artifact = listed(manifest, &format!("demo/{name}"))?;
        if file["path"] != name
            || file["bytes"] != artifact.bytes
            || file["sha256"] != artifact.sha256
            || artifact.bytes == 0
        {
            return Err(invalid("demo file identity mismatch"));
        }
    }
    let events: Value = json(dir, "demo/events.json")?;
    let commands = events["commands"]
        .as_array()
        .ok_or(invalid("demo commands"))?;
    let frames = events["frames"].as_array().ok_or(invalid("demo frames"))?;
    if events["schema"] != 1
        || events["fixture_mode"] != "owned"
        || events["started_unix_ms"] != record["started_unix_ms"]
        || commands.len() < 10
        || commands.len() > 64
        || record["commands"] != commands.len()
        || frames.is_empty()
        || frames.len() > 4096
    {
        return Err(invalid("demo recording is incomplete or exceeds limits"));
    }
    let mut outputs = vec![(String::new(), String::new()); commands.len()];
    let mut previous = 0;
    for frame in frames {
        let elapsed = number(&frame["elapsed_ms"])?;
        if elapsed < previous || elapsed > number(&record["elapsed_ms"])? {
            return Err(invalid("demo timestamps are inconsistent"));
        }
        previous = elapsed;
        let stream = text(&frame["stream"])?;
        let value = text(&frame["text"])?;
        if stream == "runner" {
            continue;
        }
        let index =
            usize::try_from(number(&frame["command"])?).map_err(|_| invalid("frame command"))?;
        let output = outputs
            .get_mut(index)
            .ok_or(invalid("frame command index"))?;
        match stream {
            "stdout" => output.0.push_str(value),
            "stderr" => output.1.push_str(value),
            _ => return Err(invalid("demo stream")),
        }
    }
    let cast = read(dir, "demo/terminal.cast", MAX_JSON)?;
    let cast = std::str::from_utf8(&cast).map_err(|_| invalid("cast UTF-8"))?;
    let mut lines = cast.lines();
    let header: Value = serde_json::from_str(lines.next().ok_or(invalid("empty cast"))?)?;
    if header["version"] != 2 {
        return Err(invalid("cast format"));
    }
    for frame in frames {
        let actual: Value =
            serde_json::from_str(lines.next().ok_or(invalid("cast omits frames"))?)?;
        if actual
            != serde_json::json!([
                number(&frame["elapsed_ms"])? as f64 / 1000.0,
                "o",
                frame["text"]
            ])
        {
            return Err(invalid("cast is not the exact recorded terminal timeline"));
        }
    }
    if lines.next().is_some() {
        return Err(invalid("cast has invented frames"));
    }
    let mut stages = BTreeMap::new();
    let mut last_end = 0;
    for (index, (command, (stdout, stderr))) in commands.iter().zip(outputs).enumerate() {
        let argv = command["argv"]
            .as_array()
            .ok_or(invalid("demo command arguments"))?;
        let name = argv
            .first()
            .and_then(Value::as_str)
            .ok_or(invalid("demo command"))?;
        let start = number(&command["started_ms"])?;
        let end = number(&command["ended_ms"])?;
        let envelope: Value = serde_json::from_str(text(&command["stdout"])?)?;
        let exit = command["exit_code"].as_i64().ok_or(invalid("demo exit"))?;
        if command["index"] != index
            || start < last_end
            || start > end
            || end > number(&record["elapsed_ms"])?
            || command["stdout"] != stdout
            || command["stderr"] != stderr
            || envelope != command["envelope"]
            || envelope["schema"] != 1
            || envelope["command"] != name
            || !matches!(exit, 0 | 3)
            || envelope["status"] != if exit == 3 { "unknown" } else { "success" }
            || envelope.get("error").is_some()
        {
            return Err(invalid("demo command recording is inconsistent"));
        }
        last_end = end;
        if name == "run" && envelope["data"]["scenario"] == "checkout" {
            let phase = text(&envelope["data"]["phase"])?;
            if !["prepared", "unknown", "completed"].contains(&phase)
                || stages.insert(phase.to_owned(), index).is_some()
                || envelope["data"]["run_id"] != record["checkout"]["run_id"]
                || envelope["data"]["operation"] != record["checkout"]["operation"]
                || (phase != "prepared"
                    && envelope["data"]["intent"] != record["checkout"]["intent"])
            {
                return Err(invalid("checkout timeline changed logical identity"));
            }
        }
        if name == "compare" {
            stages.insert("compare".into(), index);
        }
        if ["replay", "verify"].contains(&name)
            && (envelope["data"]["worker_calls"] != 0 || envelope["data"]["dispatch_calls"] != 0)
        {
            return Err(invalid("recorded replay called external execution"));
        }
    }
    let order = ["prepared", "compare", "unknown", "completed"].map(|name| {
        stages
            .get(name)
            .copied()
            .ok_or(invalid("required demo phase is missing"))
    });
    let mut last = None;
    for position in order {
        let position = position?;
        if last.is_some_and(|old| old >= position) {
            return Err(invalid("demo phases are out of order"));
        }
        last = Some(position);
    }
    let runs = events["runs"].as_array().ok_or(invalid("demo runs"))?;
    if runs.len() != 2 || record["checkout"]["prepared_branches"] != 5 {
        return Err(invalid("demo scenario evidence"));
    }
    for (run, label) in runs.iter().zip(["checkout", "data_change"]) {
        let summary = &record[label];
        if run["phase"] != "completed"
            || run["run_id"] != summary["run_id"]
            || summary["configured_worker_limit"] != 2
            || number(&summary["observed_peak"])? > 2
            || run["summary"]["peak_resident"] != summary["observed_peak"]
            || run["summary"]["logical_nodes"] != summary["logical_nodes"]
            || run["summary"]["executed"] != summary["executed"]
            || run["summary"]["reused"] != summary["reused"]
        {
            return Err(invalid(
                "demo counters are not the actual recorded run counters",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Candidate {
        _temporary: tempfile::TempDir,
        dir: std::path::PathBuf,
        manifest: ReleaseManifest,
    }
    impl Candidate {
        fn refresh(&mut self) -> Result<()> {
            for artifact in &mut self.manifest.artifacts {
                let (bytes, sha256) = manifest::hash_file(&self.dir.join(&artifact.path))?;
                artifact.bytes = bytes;
                artifact.sha256 = sha256;
            }
            let checksums: String = self
                .manifest
                .artifacts
                .iter()
                .map(|a| format!("{}  {}\n", a.sha256, a.path))
                .collect();
            fs::write(self.dir.join("SHA256SUMS"), checksums)?;
            fs::write(
                self.dir.join("manifest.json"),
                serde_json::to_vec(&self.manifest)?,
            )?;
            Ok(())
        }
    }
    fn put(dir: &Path, path: &str, bytes: impl AsRef<[u8]>) -> Result<()> {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
        Ok(())
    }
    fn put_json(dir: &Path, path: &str, value: &impl Serialize) -> Result<()> {
        put(dir, path, serde_json::to_vec(value)?)
    }
    fn envelope(command: &str, data: Value) -> Value {
        serde_json::json!({"schema":1,"command":command,"status":"success","data":data})
    }
    /// Synthetic consistency evidence only. The invalid executable deliberately
    /// proves the verifier never launches candidate payloads.
    fn candidate() -> Result<Candidate> {
        let temporary = tempfile::tempdir()?;
        let dir = temporary.path().join("candidate with spaces");
        fs::create_dir(&dir)?;
        let source = manifest::SourceRevision {
            revision: "a".repeat(40),
            dirty: false,
        };
        let lock = manifest::hash_bytes(b"synthetic lock fixture");
        let profile = "b".repeat(64);
        put(temporary.path(), "Cargo.lock", b"synthetic lock fixture")?;
        put(
            temporary.path(),
            "LICENSE-MIT",
            b"synthetic license fixture",
        )?;
        put(
            temporary.path(),
            "LICENSE-APACHE",
            b"synthetic license fixture",
        )?;
        archive::write_files(
            &dir.join("source.tar"),
            &["Cargo.lock", "LICENSE-APACHE", "LICENSE-MIT"]
                .map(|p| (p.into(), temporary.path().join(p))),
        )?;
        put(
            temporary.path(),
            "not-executable",
            b"This fixture must never be executed.",
        )?;
        archive::write_files(
            &dir.join("platform.tar"),
            &[(
                "bin/attow.exe".into(),
                temporary.path().join("not-executable"),
            )],
        )?;
        let (binary_bytes, binary_hash) =
            manifest::hash_file(&temporary.path().join("not-executable"))?;
        let target = "x86_64-pc-windows-msvc";
        let compiler = "rustc 1.95.0\nhost: x86_64-pc-windows-msvc\nrelease: 1.95.0\n";
        let mut gates = vec![
            serde_json::json!({"command":"verify-repository","exit_code":0}),
            serde_json::json!({"argv":["cargo-deny","--locked","--offline","check","licenses"],"version":"0.20.2","exit_code":0}),
            serde_json::json!({"argv":["cargo-deny","--locked","--offline","--target",target,"check","advisories"],"version":"0.20.2","scope":"actual native compiler host only","exit_code":0}),
        ];
        for argv in crate::commands::gate_plan() {
            gates.push(serde_json::json!({"argv":argv,"exit_code":0}));
        }
        gates.extend([
            serde_json::json!({"command":"demo","exit_code":0}),
            serde_json::json!({"command":"install-smoke","exit_code":0}),
        ]);
        let mut build_checks = vec![manifest::Check {
            argv: vec![
                "build",
                "-p",
                "attow-cli",
                "--release",
                "--features",
                "browser",
                "--locked",
                "-j",
                "2",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            exit_code: 0,
            evidence: "evidence/native-build.log".into(),
        }];
        put(
            &dir,
            "evidence/native-build.log",
            b"synthetic test-only build evidence",
        )?;
        for gate in &gates {
            let argv = gate["argv"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_str().expect("fixture arg").to_owned())
                        .collect()
                })
                .unwrap_or_else(|| vec![gate["command"].as_str().expect("fixture command").into()]);
            build_checks.push(manifest::Check {
                argv,
                exit_code: 0,
                evidence: "evidence/release-check/report.json".into(),
            });
        }
        put_json(
            &dir,
            "evidence/release-check/report.json",
            &serde_json::json!({"schema":1,"status":"success","source_before":source,"source_after":source,"lock_sha256_before":lock,"lock_sha256_after":lock,"checks":gates}),
        )?;
        let runtime = envelope(
            "doctor",
            serde_json::json!({"runtime_profile_hash":profile,"offline_core":true}),
        );
        put_json(&dir, "runtime-profile.json", &runtime)?;
        let run_id = "c".repeat(64);
        let installed = [
            ("--help", vec!["--help"], Value::Null),
            ("doctor", vec!["doctor", "--json"], runtime),
            (
                "init",
                vec!["init", ".", "--json"],
                envelope("init", serde_json::json!({})),
            ),
            (
                "run",
                vec!["run", "data-change", "--json"],
                envelope(
                    "run",
                    serde_json::json!({"run_id":run_id,"phase":"completed"}),
                ),
            ),
            (
                "replay",
                vec!["replay", &run_id, "--offline", "--json"],
                envelope(
                    "replay",
                    serde_json::json!({"run_id":run_id,"worker_calls":0,"dispatch_calls":0}),
                ),
            ),
            (
                "verify",
                vec!["verify", &run_id, "--json"],
                envelope(
                    "verify",
                    serde_json::json!({"run_id":run_id,"worker_calls":0,"dispatch_calls":0}),
                ),
            ),
        ];
        let mut install_checks = Vec::new();
        for (i, (name, argv, value)) in installed.into_iter().enumerate() {
            let evidence = format!("evidence/install-{i}.txt");
            if name == "--help" {
                put(&dir, &evidence, b"Attow Network synthetic help")?;
            } else {
                put_json(&dir, &evidence, &value)?;
            }
            install_checks.push(manifest::Check {
                argv: argv.into_iter().map(str::to_owned).collect(),
                exit_code: 0,
                evidence,
            });
        }
        put_json(
            &dir,
            "evidence/archive-install.json",
            &InstallEvidence {
                schema: 1,
                target: target.into(),
                archive_path: "platform.tar".into(),
                archive_sha256: manifest::hash_file(&dir.join("platform.tar"))?.1,
                binary_path: "bin/attow.exe".into(),
                binary_sha256: binary_hash.clone(),
                profile: "release".into(),
                source: source.clone(),
                checks: install_checks,
            },
        )?;
        let checkout = serde_json::json!({"run_id":"d".repeat(64),"scenario":"checkout","phase":"completed","operation":"e".repeat(64),"intent":"f".repeat(64),"summary":{"peak_resident":2,"logical_nodes":15,"executed":7,"reused":8}});
        let data = serde_json::json!({"run_id":run_id,"scenario":"data-change","phase":"completed","summary":{"peak_resident":2,"logical_nodes":10,"executed":2,"reused":8}});
        let mut prepared = checkout.clone();
        prepared["phase"] = "prepared".into();
        prepared["intent"] = Value::Null;
        let mut unknown = checkout.clone();
        unknown["phase"] = "unknown".into();
        let mut unknown_envelope = envelope("run", unknown);
        unknown_envelope["status"] = "unknown".into();
        let mut commands = Vec::new();
        let mut frames = Vec::new();
        for (index, (name, code, value)) in [
            ("init", 0, envelope("init", serde_json::json!({}))),
            ("run", 0, envelope("run", prepared)),
            ("branch", 0, envelope("branch", serde_json::json!({}))),
            ("compare", 0, envelope("compare", serde_json::json!({}))),
            ("run", 3, unknown_envelope),
            ("run", 0, envelope("run", checkout.clone())),
            (
                "replay",
                0,
                envelope(
                    "replay",
                    serde_json::json!({"worker_calls":0,"dispatch_calls":0}),
                ),
            ),
            (
                "verify",
                0,
                envelope(
                    "verify",
                    serde_json::json!({"worker_calls":0,"dispatch_calls":0}),
                ),
            ),
            ("run", 0, envelope("run", data.clone())),
            (
                "replay",
                0,
                envelope(
                    "replay",
                    serde_json::json!({"worker_calls":0,"dispatch_calls":0}),
                ),
            ),
            (
                "verify",
                0,
                envelope(
                    "verify",
                    serde_json::json!({"worker_calls":0,"dispatch_calls":0}),
                ),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let stdout = serde_json::to_string(&value)?;
            let argv=match index {
                1=>serde_json::json!(["run","checkout","--pause-after-prepared","--json"]),
                4|5=>serde_json::json!(["run","checkout","--resume","d".repeat(64),"--json"]),
                6=>serde_json::json!(["replay","d".repeat(64),"--offline","--json"]),
                7=>serde_json::json!(["verify","d".repeat(64),"--json"]),
                8=>serde_json::json!(["run","data-change","--json"]),
                9=>serde_json::json!(["replay",run_id,"--offline","--json"]),
                10=>serde_json::json!(["verify",run_id,"--json"]),
                _=>serde_json::json!([name]),
            };
            commands.push(serde_json::json!({"index":index,"argv":argv,"started_ms":index*10,"ended_ms":index*10+5,"exit_code":code,"stdout":stdout,"stderr":"","envelope":value}));
            frames.push(serde_json::json!({"elapsed_ms":index*10+5,"stream":"stdout","command":index,"text":stdout}));
        }
        put_json(
            &dir,
            "demo/events.json",
            &serde_json::json!({"schema":1,"fixture_mode":"owned","started_unix_ms":1000,"commands":commands,"frames":frames,"runs":[checkout,data]}),
        )?;
        let mut cast = "{\"version\":2}\n".to_owned();
        for frame in &frames {
            cast.push_str(&serde_json::to_string(&serde_json::json!([
                frame["elapsed_ms"].as_u64().expect("fixture time") as f64 / 1000.0,
                "o",
                frame["text"]
            ]))?);
            cast.push('\n');
        }
        put(&dir, "demo/terminal.cast", cast)?;
        put(
            &dir,
            "demo/index.html",
            b"<!doctype html><title>synthetic test viewer</title>",
        )?;
        let files = ["events.json", "index.html", "terminal.cast"]
            .into_iter()
            .map(|name| {
                let (bytes, sha256) = manifest::hash_file(&dir.join("demo").join(name))?;
                Ok(serde_json::json!({"path":name,"bytes":bytes,"sha256":sha256}))
            })
            .collect::<Result<Vec<_>>>()?;
        put_json(
            &dir,
            "demo/recording.json",
            &serde_json::json!({"schema":1,"source":source,"executable":{"bytes":binary_bytes,"sha256":binary_hash,"profile":"release"},"started_unix_ms":1000,"elapsed_ms":200,"fixture_mode":"owned","provider":"simulated","files":files,"commands":11,"checkout":{"run_id":"d".repeat(64),"operation":"e".repeat(64),"intent":"f".repeat(64),"prepared_branches":5,"configured_worker_limit":2,"observed_peak":2,"logical_nodes":15,"executed":7,"reused":8},"data_change":{"run_id":run_id,"configured_worker_limit":2,"observed_peak":2,"logical_nodes":10,"executed":2,"reused":8}}),
        )?;
        put_json(
            &dir,
            "provenance.json",
            &serde_json::json!({"schema":1,"source":source,"target":target,"rustc_verbose":compiler,"lock_sha256":lock,"runtime_profile_hash":profile,"build_profile":"release","build_checks":build_checks}),
        )?;
        for p in [
            "sbom.cdx.json",
            "dependency-inventory.json",
            "notices/license.txt",
        ] {
            put(&dir, p, b"synthetic integrity-only fixture")?;
        }
        let mut paths = BTreeSet::new();
        inventory(&dir, &dir, &mut paths, 0)?;
        let artifacts = paths
            .into_iter()
            .map(|path| {
                let role = match path.as_str() {
                    "platform.tar" => "platform-archive",
                    "source.tar" => "source-archive",
                    "sbom.cdx.json" => "sbom",
                    "dependency-inventory.json" => "dependency-inventory",
                    "runtime-profile.json" => "runtime-profile",
                    "provenance.json" => "provenance",
                    p if p.starts_with("notices/") => "third-party-notices",
                    p if p.starts_with("demo/") => "demo",
                    _ => "evidence",
                };
                Artifact {
                    role: role.into(),
                    path,
                    bytes: 0,
                    sha256: String::new(),
                }
            })
            .collect();
        let manifest = ReleaseManifest {
            schema: 1,
            project: "Attow Network".into(),
            version: "0.1.0".into(),
            source,
            target: target.into(),
            rustc_verbose: compiler.into(),
            lock_sha256: lock,
            runtime_profile_hash: profile,
            artifacts,
            build_checks,
            expected_signing_identity: None,
            supported_features: vec![manifest::Feature {
                name: "synthetic test feature".into(),
                status: "passed".into(),
                prerequisites: vec![],
            }],
            supported_targets: vec![target.into()],
            install_evidence: "evidence/archive-install.json".into(),
            demo_evidence: "demo/recording.json".into(),
        };
        let mut candidate = Candidate {
            _temporary: temporary,
            dir,
            manifest,
        };
        candidate.refresh()?;
        Ok(candidate)
    }
    #[test]
    fn complete_candidate_is_unsigned_and_never_executes_its_invalid_binary() -> Result<()> {
        let candidate = candidate()?;
        let result = verify_candidate(&candidate.dir, VerificationMode::IntegrityOnly)?;
        assert_eq!(result.signature_status, "absent");
        assert!(!result.release_ready);
        assert!(verify_candidate(&candidate.dir, VerificationMode::RequireSignatures).is_err());
        Ok(())
    }
    fn mutate_demo(kind:usize)->Result<Candidate> {
        let mut candidate=candidate()?;
        verify_candidate(&candidate.dir,VerificationMode::IntegrityOnly)?;
        let mut events:Value=json(&candidate.dir,"demo/events.json")?;
        let mut recording:Value=json(&candidate.dir,"demo/recording.json")?;
        match kind {
            0=>{events["runs"][0]["summary"]["reused"]=9.into();recording["checkout"]["reused"]=9.into();}
            1=>{events["commands"][6]["argv"]=serde_json::json!(["doctor","--json"]);events["commands"][6]["envelope"]["command"]="doctor".into();let stdout=serde_json::to_string(&events["commands"][6]["envelope"])?;events["commands"][6]["stdout"]=stdout.clone().into();events["frames"][6]["text"]=stdout.into();}
            _=>events["commands"][0]["ended_ms"]=4.into(),
        }
        put_json(&candidate.dir,"demo/events.json",&events)?;
        let mut cast="{\"version\":2}\n".to_owned();for frame in events["frames"].as_array().expect("fixture frames") {cast.push_str(&serde_json::to_string(&serde_json::json!([frame["elapsed_ms"].as_u64().expect("fixture time") as f64/1000.0,"o",frame["text"]]))?);cast.push('\n');}put(&candidate.dir,"demo/terminal.cast",cast)?;
        for file in recording["files"].as_array_mut().expect("fixture files") {let(bytes,sha256)=manifest::hash_file(&candidate.dir.join("demo").join(file["path"].as_str().expect("fixture path")))?;file["bytes"]=bytes.into();file["sha256"]=sha256.into();}
        put_json(&candidate.dir,"demo/recording.json",&recording)?;candidate.refresh()?;Ok(candidate)
    }
    #[test]
    fn rehashed_demo_summary_must_equal_completed_command_output()->Result<()> {
        let candidate=mutate_demo(0)?;assert!(verify_candidate(&candidate.dir,VerificationMode::IntegrityOnly).is_err());Ok(())
    }
    #[test]
    fn rehashed_demo_cannot_omit_the_actual_offline_replay()->Result<()> {
        let candidate=mutate_demo(1)?;assert!(verify_candidate(&candidate.dir,VerificationMode::IntegrityOnly).is_err());Ok(())
    }
    #[test]
    fn recorded_frame_must_belong_to_its_actual_command_interval()->Result<()> {
        let candidate=mutate_demo(2)?;assert!(verify_candidate(&candidate.dir,VerificationMode::IntegrityOnly).is_err());Ok(())
    }
    #[test]
    fn empty_directories_consume_the_bounded_traversal_budget()->Result<()> {
        let dir=tempfile::tempdir()?;for i in 0..5 {fs::create_dir(dir.path().join(format!("empty-{i}")))?;}
        let mut files=BTreeSet::new();assert!(inventory_with_limit(dir.path(),dir.path(),&mut files,0,4).is_err());
        inventory_with_limit(dir.path(),dir.path(),&mut files,0,5)?;assert!(files.is_empty());Ok(())
    }
    #[test]
    fn candidate_rejects_mutated_payload_checksum_install_source_and_demo() -> Result<()> {
        for mutation in 0..7 {
            let mut candidate = candidate()?;
            verify_candidate(&candidate.dir, VerificationMode::IntegrityOnly)?;
            match mutation {
                0 => fs::write(candidate.dir.join("platform.tar"), b"mutated archive")?,
                1 => fs::write(candidate.dir.join("SHA256SUMS"), b"invented checksum list")?,
                2 => {
                    let mut install: InstallEvidence =
                        json(&candidate.dir, &candidate.manifest.install_evidence)?;
                    install.binary_sha256 = "0".repeat(64);
                    put_json(
                        &candidate.dir,
                        &candidate.manifest.install_evidence,
                        &install,
                    )?;
                    candidate.refresh()?;
                }
                3 => {
                    let mut bytes = fs::read(candidate.dir.join("source.tar"))?;
                    bytes[..100].fill(0);
                    bytes[..9].copy_from_slice(b"../escape");
                    bytes[148..156].fill(b' ');
                    let sum: u64 = bytes[..512].iter().map(|b| u64::from(*b)).sum();
                    bytes[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
                    fs::write(candidate.dir.join("source.tar"), bytes)?;
                    candidate.refresh()?;
                }
                4 => {
                    let mut record: Value =
                        json(&candidate.dir, &candidate.manifest.demo_evidence)?;
                    record["checkout"]["intent"] = "0".repeat(64).into();
                    put_json(&candidate.dir, &candidate.manifest.demo_evidence, &record)?;
                    candidate.refresh()?;
                }
                5 => {
                    candidate
                        .manifest
                        .supported_targets
                        .push("aarch64-apple-darwin".into());
                    candidate.refresh()?;
                }
                _ => put(
                    &candidate.dir,
                    "unlisted-secret",
                    b"must not be distributed",
                )?,
            }
            assert!(
                verify_candidate(&candidate.dir, VerificationMode::IntegrityOnly).is_err(),
                "mutation {mutation} was accepted"
            );
        }
        Ok(())
    }
    #[test]
    fn checksum_list_is_exact_sorted_payload_only_data() {
        let artifacts = vec![Artifact {
            path: "a.tar".into(),
            bytes: 1,
            sha256: "a".repeat(64),
            role: "source-archive".into(),
        }];
        let exact = format!("{}  a.tar\n", "a".repeat(64));
        assert!(checksums(&artifacts, exact.as_bytes()).is_ok());
        for text in [
            String::new(),
            format!("{exact}{exact}"),
            exact.replace("a.tar", "../a.tar"),
            exact.replace("a.tar", "manifest.json"),
            exact.replace('a', "b"),
            format!("{exact}{}  extra\n", "b".repeat(64)),
        ] {
            assert!(checksums(&artifacts, text.as_bytes()).is_err());
        }
    }
    #[test]
    fn manifest_cannot_claim_an_unbuilt_platform_or_failed_feature() -> Result<()> {
        let mut manifest: ReleaseManifest = serde_json::from_value(serde_json::json!({
            "schema":1,"project":"Attow Network","version":"0.1.0","source":{"revision":"a".repeat(40),"dirty":false},
            "target":"x86_64-pc-windows-msvc","rustc_verbose":"rustc 1.95.0\nhost: x86_64-pc-windows-msvc\n",
            "lock_sha256":"a".repeat(64),"runtime_profile_hash":"b".repeat(64),"artifacts":[],"build_checks":[],
            "expected_signing_identity":null,"supported_features":[{"name":"local","status":"passed","prerequisites":[]}],
            "supported_targets":["x86_64-pc-windows-msvc","aarch64-apple-darwin"],"install_evidence":"evidence/archive-install.json","demo_evidence":"demo/recording.json"
        }))?;
        assert!(validate_manifest(&manifest).is_err());
        manifest.supported_targets.pop();
        manifest.supported_features[0].status = "failed".into();
        assert!(validate_manifest(&manifest).is_err());
        Ok(())
    }
}
