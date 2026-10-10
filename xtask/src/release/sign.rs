//! Detached signatures over the frozen manifest, with externally configured trust.
use super::{
    ReleaseError, Result,
    manifest::{self, ExpectedSigningIdentity, ReleaseManifest},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const CONFIG_ENV: &str = "ATTOW_RELEASE_SIGN_CONFIG";
pub const BUNDLE_NAME: &str = "manifest.sigstore.json";
pub const COSIGN_WINDOWS_SHA256: &str =
    "9fe59be0eca1271873ce019061335eb1ac419b7059202e797828467ddabe33be";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignConfig {
    pub schema: u32,
    pub cosign: PathBuf,
    pub cosign_sha256: String,
    pub trust: Trust,
    pub require_transparency: bool,
    pub signing: Option<SigningAuthority>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trust {
    Oidc { identity: String, issuer: String },
    PublicKey { path: PathBuf, sha256: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SigningAuthority {
    Key {
        key_env: String,
        allow_transparency: bool,
    },
    Oidc {
        provider: String,
        allow_transparency: bool,
    },
}

/// Extract independently configured public trust before the manifest is frozen.
/// This does not require or generate a private key.
pub fn configured_identity(dir: &Path) -> Result<Option<ExpectedSigningIdentity>> {
    if std::env::var_os(CONFIG_ENV).is_none() {
        return Ok(None);
    }
    let config = configured(dir)?;
    let identity = match &config.trust {
        Trust::Oidc { identity, issuer } => ExpectedSigningIdentity::Oidc {
            identity: identity.clone(),
            issuer: issuer.clone(),
        },
        Trust::PublicKey { path, sha256 } => {
            if manifest::hash_file(path)?.1 != *sha256 {
                return Err(ReleaseError::Invalid("public key integrity mismatch"));
            }
            ExpectedSigningIdentity::PublicKey {
                sha256: sha256.clone(),
            }
        }
    };
    validate_trust(&config, Some(&identity))?;
    verified_tool(&config)?;
    Ok(Some(identity))
}

pub fn sign_configured(dir: &Path) -> Result<super::verify::VerificationReport> {
    let config = configured(dir)?;
    if config.signing.is_none() {
        return Err(ReleaseError::Invalid(
            "signing credentials are not configured; candidate remains unsigned",
        ));
    }
    let before =
        super::verify::verify_candidate(dir, super::verify::VerificationMode::IntegrityOnly)?;
    let (manifest, raw) = manifest::load(dir)?;
    validate_trust(&config, manifest.expected_signing_identity.as_ref())?;
    let destination = dir.join(BUNDLE_NAME);
    if destination.exists() {
        return Err(ReleaseError::Invalid("detached signature already exists"));
    }
    let tool = verified_tool(&config)?;
    // Staging lives outside the distribution and is removed on every failed path.
    let temporary = tempfile::tempdir()?;
    let payload = temporary.path().join("manifest.json");
    let bundle = temporary.path().join(BUNDLE_NAME);
    fs::write(&payload, &raw)?;
    let mut args: Vec<OsString> = vec![
        "sign-blob".into(),
        payload.as_os_str().into(),
        "--bundle".into(),
        bundle.as_os_str().into(),
        "--yes".into(),
    ];
    match config
        .signing
        .as_ref()
        .ok_or(ReleaseError::Invalid("missing signing authority"))?
    {
        SigningAuthority::Key {
            key_env,
            allow_transparency,
        } => {
            let reference = std::env::var_os(key_env).filter(|v| !v.is_empty()).ok_or(
                ReleaseError::Invalid(
                    "configured signing credential is absent; candidate remains unsigned",
                ),
            )?;
            args.extend([
                "--key".into(),
                reference,
                "--use-signing-config=false".into(),
            ]);
            if !allow_transparency {
                args.push("--tlog-upload=false".into());
            }
        }
        SigningAuthority::Oidc { provider, .. } => {
            args.extend([
                format!("--oidc-provider={provider}").into(),
                "--fulcio-auth-flow=token".into(),
            ]);
        }
    }
    tool_run(&tool, &args, &[])?;
    verify_bundle(&config, &tool, &payload, &bundle)?;
    let after =
        super::verify::verify_candidate(dir, super::verify::VerificationMode::IntegrityOnly)?;
    if before.manifest_sha256 != after.manifest_sha256 || manifest::load(dir)?.1 != raw {
        return Err(ReleaseError::Invalid("candidate changed during signing"));
    }
    let signature = bundle_bytes(&bundle)?;
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    out.write_all(&signature)?;
    out.sync_all()?;
    super::verify::verify_candidate(dir, super::verify::VerificationMode::RequireSignatures)
}
pub(super) fn verify_manifest_signature(
    dir: &Path,
    manifest: &ReleaseManifest,
    raw: &[u8],
) -> Result<()> {
    let config = configured(dir)?;
    validate_trust(&config, manifest.expected_signing_identity.as_ref())?;
    let bundle = manifest::artifact_path(dir, BUNDLE_NAME)?;
    let signature = bundle_bytes(&bundle)?;
    let tool = verified_tool(&config)?;
    let temp = tempfile::tempdir()?;
    let payload = temp.path().join("manifest.json");
    let bundle = temp.path().join(BUNDLE_NAME);
    fs::write(&payload, raw)?;
    fs::write(&bundle, signature)?;
    verify_bundle(&config, &tool, &payload, &bundle)
}
fn configured(dir: &Path) -> Result<SignConfig> {
    let config = std::env::var_os(CONFIG_ENV).ok_or(ReleaseError::Invalid(
        "external signing trust is not configured; signing remains pending",
    ))?;
    load_config(dir, Path::new(&config))
}
fn load_config(dir: &Path, path: &Path) -> Result<SignConfig> {
    if !path.is_absolute() {
        return Err(ReleaseError::Invalid(
            "signing configuration must be an absolute external path",
        ));
    }
    external_file(dir, path)?;
    let config: SignConfig = serde_json::from_slice(&read_bounded(path, 64 * 1024)?)?;
    external_file(dir, &config.cosign)?;
    if let Trust::PublicKey { path, .. } = &config.trust {
        external_file(dir, path)?;
    }
    Ok(config)
}
fn external_file(dir: &Path, path: &Path) -> Result<()> {
    if !path.is_absolute() || path.canonicalize()?.starts_with(dir.canonicalize()?) {
        return Err(ReleaseError::Invalid(
            "candidate files cannot establish signature trust",
        ));
    }
    for parent in path.ancestors() {
        manifest::reject_link(parent)?;
    }
    if !fs::metadata(path)?.is_file() {
        return Err(ReleaseError::Invalid(
            "external trust must be a regular file",
        ));
    }
    Ok(())
}
fn plain(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate_trust(config: &SignConfig, expected: Option<&ExpectedSigningIdentity>) -> Result<()> {
    if config.schema != 1 || config.cosign_sha256 != COSIGN_WINDOWS_SHA256 {
        return Err(ReleaseError::Invalid(
            "unsupported signing configuration or unpinned tool",
        ));
    }
    let matches = match (&config.trust, expected) {
        (
            Trust::Oidc { identity, issuer },
            Some(ExpectedSigningIdentity::Oidc {
                identity: wanted,
                issuer: wanted_issuer,
            }),
        ) => {
            plain(identity)
                && plain(issuer)
                && issuer.starts_with("https://")
                && identity == wanted
                && issuer == wanted_issuer
                && config.require_transparency
        }
        (
            Trust::PublicKey { sha256, .. },
            Some(ExpectedSigningIdentity::PublicKey { sha256: wanted }),
        ) => hex(sha256) && sha256 == wanted,
        _ => false,
    };
    if !matches {
        return Err(ReleaseError::Invalid(
            "candidate signing identity does not match external trust",
        ));
    }
    if let Some(authority) = &config.signing {
        match (&config.trust, authority) {
            (
                Trust::PublicKey { .. },
                SigningAuthority::Key {
                    key_env,
                    allow_transparency,
                },
            ) if !key_env.is_empty()
                && key_env.len() <= 64
                && key_env
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                && (!config.require_transparency || *allow_transparency) => {}
            (
                Trust::Oidc { .. },
                SigningAuthority::Oidc {
                    provider,
                    allow_transparency: true,
                },
            ) if provider == "github-actions" || provider == "filesystem" => {}
            _ => {
                return Err(ReleaseError::Invalid(
                    "signing authority or transparency permission does not match trust",
                ));
            }
        }
    }
    Ok(())
}
fn verified_tool(config: &SignConfig) -> Result<PathBuf> {
    if manifest::hash_file(&config.cosign)?.1 != COSIGN_WINDOWS_SHA256 {
        return Err(ReleaseError::Invalid(
            "Cosign executable integrity mismatch",
        ));
    }
    let output = tool_run(&config.cosign, &["version".into(), "--json".into()], &[])?;
    let version: serde_json::Value = serde_json::from_slice(&output)?;
    if version["gitVersion"] != "v3.1.3"
        || version["gitCommit"] != "11926fa5bbbbde47e88fc006b625a17769b743b2"
        || version["platform"] != "windows/amd64"
    {
        return Err(ReleaseError::Invalid(
            "Cosign version is not the verified pinned build",
        ));
    }
    Ok(config.cosign.clone())
}
fn verify_bundle(config: &SignConfig, tool: &Path, payload: &Path, bundle: &Path) -> Result<()> {
    bundle_bytes(bundle)?;
    let mut args = vec![
        "verify-blob".into(),
        payload.as_os_str().to_owned(),
        "--bundle".into(),
        bundle.as_os_str().to_owned(),
    ];
    match &config.trust {
        Trust::PublicKey { path, sha256 } => {
            if manifest::hash_file(path)?.1 != *sha256 {
                return Err(ReleaseError::Invalid("public key integrity mismatch"));
            }
            args.extend(["--key".into(), path.as_os_str().to_owned()]);
            if !config.require_transparency {
                args.push("--insecure-ignore-tlog".into());
            }
        }
        Trust::Oidc { identity, issuer } => args.extend([
            format!("--certificate-identity={identity}").into(),
            format!("--certificate-oidc-issuer={issuer}").into(),
        ]),
    }
    tool_run(tool, &args, &[])?;
    Ok(())
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    manifest::reject_link(path)?;
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(ReleaseError::Invalid("signature input exceeds limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(ReleaseError::Invalid("signature input exceeds limit"));
    }
    Ok(bytes)
}
fn bundle_bytes(path: &Path) -> Result<Vec<u8>> {
    let bytes = read_bounded(path, 1024 * 1024)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if value["mediaType"] != "application/vnd.dev.sigstore.bundle.v0.3+json"
        || !value["verificationMaterial"].is_object()
        || value["messageSignature"]["messageDigest"]["algorithm"] != "SHA2_256"
        || value["messageSignature"]["signature"]
            .as_str()
            .is_none_or(str::is_empty)
        || value["messageSignature"]["messageDigest"]["digest"]
            .as_str()
            .is_none_or(str::is_empty)
    {
        return Err(ReleaseError::Invalid(
            "unsupported or malformed detached signature bundle",
        ));
    }
    Ok(bytes)
}
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn tool_run(tool: &Path, args: &[OsString], env: &[(&str, &str)]) -> Result<Vec<u8>> {
    let stdout = tempfile::NamedTempFile::new()?;
    let stderr = tempfile::NamedTempFile::new()?;
    let mut command = Command::new(tool);
    command
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.reopen()?))
        .stderr(Stdio::from(stderr.reopen()?));
    let mut child = Running(command.spawn()?);
    let start = Instant::now();
    let mut update = Duration::from_secs(30);
    loop {
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                return Err(ReleaseError::Tool(
                    "pinned Cosign operation failed; candidate is not authenticated".into(),
                ));
            }
            return read_bounded(stdout.path(), 1024 * 1024);
        }
        if start.elapsed() > Duration::from_secs(180)
            || stdout.as_file().metadata()?.len() > 1024 * 1024
            || stderr.as_file().metadata()?.len() > 1024 * 1024
        {
            return Err(ReleaseError::Tool(
                "Cosign exceeded its time or output bound".into(),
            ));
        }
        if start.elapsed() >= update {
            eprintln!("Attow Network signing-tool operation is still running.");
            update += Duration::from_secs(30);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> SignConfig {
        SignConfig {
            schema: 1,
            cosign: PathBuf::from("tool"),
            cosign_sha256: COSIGN_WINDOWS_SHA256.into(),
            trust: Trust::Oidc {
                identity:
                    "https://github.com/owner/project/.github/workflows/release.yml@refs/tags/v1"
                        .into(),
                issuer: "https://token.actions.githubusercontent.com".into(),
            },
            require_transparency: true,
            signing: None,
        }
    }
    #[test]
    fn candidate_identity_cannot_establish_its_own_trust() {
        let config = config();
        assert!(validate_trust(&config, None).is_err());
        assert!(
            validate_trust(
                &config,
                Some(&ExpectedSigningIdentity::Oidc {
                    identity: "attacker".into(),
                    issuer: "https://token.actions.githubusercontent.com".into()
                })
            )
            .is_err()
        );
        assert!(
            validate_trust(
                &config,
                Some(&ExpectedSigningIdentity::PublicKey {
                    sha256: "a".repeat(64)
                })
            )
            .is_err()
        );
    }
    #[test]
    fn malformed_signature_bundle_is_not_verification_evidence() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(BUNDLE_NAME);
        std::fs::write(&path, b"{\"signature\":\"invented\"}")?;
        assert!(bundle_bytes(&path).is_err());
        std::fs::write(&path, b"not-json")?;
        assert!(bundle_bytes(&path).is_err());
        Ok(())
    }
    #[test]
    fn candidate_local_trust_file_is_never_accepted() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sign-config.json");
        fs::write(&path, serde_json::to_vec(&config())?)?;
        assert!(load_config(dir.path(), &path).is_err());
        assert_eq!(fs::read_dir(dir.path())?.count(), 1);
        assert!(!dir.path().join(BUNDLE_NAME).exists());
        Ok(())
    }
    #[test]
    #[ignore = "required separate Stage 10 gate: provision pinned Cosign 3.1.3; temporary test keys only, no upload"]
    fn pinned_cosign_authenticates_real_signature_and_rejects_mutations() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let tool = std::env::var_os("ATTOW_TEST_COSIGN")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .expect("workspace root")
                    .join(".tools/cosign-3.1.3/cosign-windows-amd64.exe")
            });
        let mut config = config();
        config.cosign = tool.clone();
        config.require_transparency = false;
        verified_tool(&config)?;
        let key = dir.path().join("isolated-test-key");
        let password = [("COSIGN_PASSWORD", "explicitly-isolated-test-only")];
        tool_run(
            &tool,
            &[
                "generate-key-pair".into(),
                "--output-key-prefix".into(),
                key.as_os_str().into(),
            ],
            &password,
        )?;
        let public = key.with_extension("pub");
        let private = key.with_extension("key");
        config.trust = Trust::PublicKey {
            sha256: manifest::hash_file(&public)?.1,
            path: public,
        };
        let payload = dir.path().join("test-manifest.json");
        let bundle = dir.path().join(BUNDLE_NAME);
        fs::write(&payload, b"explicit test payload, never a release identity")?;
        tool_run(
            &tool,
            &[
                "sign-blob".into(),
                payload.as_os_str().into(),
                "--key".into(),
                private.as_os_str().into(),
                "--bundle".into(),
                bundle.as_os_str().into(),
                "--use-signing-config=false".into(),
                "--tlog-upload=false".into(),
                "--yes".into(),
            ],
            &password,
        )?;
        verify_bundle(&config, &tool, &payload, &bundle)?;
        let original = fs::read(&payload)?;
        fs::write(&payload, b"changed manifest")?;
        assert!(verify_bundle(&config, &tool, &payload, &bundle).is_err());
        fs::write(&payload, original)?;
        let original = fs::read(&bundle)?;
        let mut value: serde_json::Value = serde_json::from_slice(&original)?;
        let signature = value["messageSignature"]["signature"]
            .as_str()
            .ok_or(ReleaseError::Invalid("test signature"))?;
        value["messageSignature"]["signature"] = format!("A{}", &signature[1..]).into();
        fs::write(&bundle, serde_json::to_vec(&value)?)?;
        assert!(verify_bundle(&config, &tool, &payload, &bundle).is_err());
        fs::write(&bundle, original)?;
        let wrong = dir.path().join("different-test-key");
        tool_run(
            &tool,
            &[
                "generate-key-pair".into(),
                "--output-key-prefix".into(),
                wrong.as_os_str().into(),
            ],
            &password,
        )?;
        let public = wrong.with_extension("pub");
        config.trust = Trust::PublicKey {
            sha256: manifest::hash_file(&public)?.1,
            path: public,
        };
        assert!(verify_bundle(&config, &tool, &payload, &bundle).is_err());
        Ok(())
    }
    #[test]
    fn trust_configuration_rejects_unknown_fields_and_invalid_schema() {
        let mut value = serde_json::to_value(config()).expect("test config");
        value["allow_unverified"] = true.into();
        assert!(serde_json::from_value::<SignConfig>(value).is_err());
        let mut config = config();
        config.schema = 2;
        assert!(validate_trust(&config, Some(&ExpectedSigningIdentity::Oidc { identity: "https://github.com/owner/project/.github/workflows/release.yml@refs/tags/v1".into(), issuer: "https://token.actions.githubusercontent.com".into() })).is_err());
    }
}
