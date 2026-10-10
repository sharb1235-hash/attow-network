use super::{ReleaseError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRevision {
    pub revision: String,
    pub dirty: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub role: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub evidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedSigningIdentity {
    Oidc { identity: String, issuer: String },
    PublicKey { sha256: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub name: String,
    pub status: String,
    pub prerequisites: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema: u32,
    pub project: String,
    pub version: String,
    pub source: SourceRevision,
    pub target: String,
    pub rustc_verbose: String,
    pub lock_sha256: String,
    pub runtime_profile_hash: String,
    pub artifacts: Vec<Artifact>,
    pub build_checks: Vec<Check>,
    pub expected_signing_identity: Option<ExpectedSigningIdentity>,
    pub supported_features: Vec<Feature>,
    pub supported_targets: Vec<String>,
    pub install_evidence: String,
    pub demo_evidence: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallEvidence {
    pub schema: u32,
    pub target: String,
    pub archive_path: String,
    pub archive_sha256: String,
    pub binary_path: String,
    pub binary_sha256: String,
    pub profile: String,
    pub source: SourceRevision,
    pub checks: Vec<Check>,
}
pub fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((bytes, format!("{:x}", hash.finalize())))
}
pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn safe_relative(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 240
        || !value.is_ascii()
        || value
            .bytes()
            .any(|b| b < 32 || b == 127 || b"\\:*?\"<>|".contains(&b))
    {
        return Err(ReleaseError::Invalid("non-portable artifact path"));
    }
    for part in value.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(ReleaseError::Invalid("non-portable artifact component"));
        }
    }
    Ok(())
}
pub fn artifact_path(dir: &Path, value: &str) -> Result<PathBuf> {
    safe_relative(value)?;
    let path = dir.join(value);
    let mut cursor = dir.to_path_buf();
    reject_link(&cursor)?;
    for part in value.split('/') {
        cursor.push(part);
        reject_link(&cursor)?;
    }
    if !fs::metadata(&path)?.is_file() {
        return Err(ReleaseError::Invalid("artifact is not a regular file"));
    }
    Ok(path)
}
pub fn reject_link(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() {
        return Err(ReleaseError::Invalid("symbolic links are forbidden"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(ReleaseError::Invalid("reparse points are forbidden"));
        }
    }
    Ok(())
}
pub fn load(dir: &Path) -> Result<(ReleaseManifest, Vec<u8>)> {
    let path = artifact_path(dir, "manifest.json")?;
    if fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
        return Err(ReleaseError::Invalid("manifest exceeds limit"));
    }
    let bytes = fs::read(path)?;
    let manifest = serde_json::from_slice(&bytes)?;
    Ok((manifest, bytes))
}
pub fn source_revision(root: &Path) -> Result<SourceRevision> {
    fn git(root: &Path, args: &[&str]) -> Result<String> {
        let out = Command::new("git").args(args).current_dir(root).output()?;
        if !out.status.success() {
            return Err(ReleaseError::Tool("git source inspection failed".into()));
        }
        String::from_utf8(out.stdout).map_err(|_| ReleaseError::Invalid("non-UTF8 git output"))
    }
    let revision = git(root, &["rev-parse", "HEAD"])?.trim().to_owned();
    if revision.len() != 40 || !revision.bytes().all(|x| x.is_ascii_hexdigit()) {
        return Err(ReleaseError::Invalid("source revision"));
    }
    Ok(SourceRevision {
        revision,
        dirty: !git(root, &["status", "--porcelain", "--untracked-files=normal"])?
            .trim()
            .is_empty(),
    })
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_paths_reject_escapes_and_windows_aliases() {
        for path in [
            "",
            "../secret",
            "/root",
            "C:/root",
            "a\\b",
            "x/../y",
            "x//y",
            "NUL.txt",
            "a/COM1",
            "a./b",
            "a /b",
            "a\nb",
            "a:b",
            "x/.",
        ] {
            assert!(safe_relative(path).is_err(), "accepted {path:?}");
        }
        assert!(safe_relative("demos/workflow-studio/browser/sidecar.mjs").is_ok());
    }
    #[test]
    fn hashes_stream_actual_bytes_and_json_is_create_only() -> Result<()> {
        let t = tempfile::tempdir()?;
        let p = t.path().join("test.json");
        write_json(&p, &serde_json::json!({"schema":1}))?;
        assert!(write_json(&p, &serde_json::json!({"schema":2})).is_err());
        let bytes = fs::read(&p)?;
        assert_eq!(hash_file(&p)?, (bytes.len() as u64, hash_bytes(&bytes)));
        Ok(())
    }
}
