//! Offline dependency metadata and exact upstream notice inventory.
use super::manifest::{hash_file, safe_relative};
use super::{ReleaseError as Error, Result};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_METADATA: u64 = 32 * 1024 * 1024;
const MAX_PACKAGES: usize = 2048;
const MAX_FILES: usize = 32768;
const MAX_NOTICE: u64 = 32 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024;

pub struct InventoryOutput {
    pub artifacts: Vec<String>,
}

pub fn generate(root: &Path, target: &str, output: &Path) -> Result<InventoryOutput> {
    if target != "x86_64-pc-windows-msvc" {
        return Err(Error::Invalid("unqualified inventory target"));
    }
    let mut all = metadata(root, None)?;
    verify_archives(root, &mut all)?;
    let tool = Command::new("tar").arg("--version").output()?;
    if !tool.status.success() || tool.stdout.len() > 8192 {
        return Err(Error::Tool("tar version probe failed".into()));
    }
    all["attow_notice_tool"] = json!({"program":"tar","version_output":String::from_utf8(tool.stdout).map_err(|_|Error::Invalid("tar version UTF-8"))?.trim(),"purpose":"Read selected notice members after verifying registry archive SHA-256 against Cargo.lock; no code extraction or execution"});
    let filtered = metadata(root, Some(target))?;
    collect(root, target, &all, &filtered, output)
}

fn lock_checksums(bytes: &str) -> Result<BTreeMap<(String, String), String>> {
    let mut result = BTreeMap::new();
    for section in bytes.split("[[package]]").skip(1) {
        let mut fields = BTreeMap::new();
        for line in section.lines() {
            if let Some((key, value)) = line.split_once(" = ")
                && ["name", "version", "source", "checksum"].contains(&key)
            {
                let value: String = serde_json::from_str(value)?;
                if fields.insert(key, value).is_some() {
                    return Err(Error::Invalid("duplicate lock field"));
                }
            }
        }
        if let Some(checksum) = fields.get("checksum") {
            if checksum.len() != 64 || !checksum.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::Invalid("lock checksum"));
            }
            let name = fields.get("name").ok_or(Error::Invalid("lock name"))?;
            let version = fields
                .get("version")
                .ok_or(Error::Invalid("lock version"))?;
            if result
                .insert((name.clone(), version.clone()), checksum.clone())
                .is_some()
            {
                return Err(Error::Invalid("duplicate lock package"));
            }
        }
    }
    Ok(result)
}

fn verify_archives(root: &Path, metadata: &mut Value) -> Result<()> {
    let lock = fs::read_to_string(root.join("Cargo.lock"))?;
    if lock.len() > 4 * 1024 * 1024 {
        return Err(Error::Invalid("lock size bound"));
    }
    let checksums = lock_checksums(&lock)?;
    let packages = metadata["packages"]
        .as_array_mut()
        .ok_or(Error::Invalid("metadata packages"))?;
    for package in packages {
        if package["source"].is_null() {
            continue;
        }
        let name = text(package, "name")?;
        let version = text(package, "version")?;
        let expected = checksums
            .get(&(name.into(), version.into()))
            .ok_or(Error::Invalid("package absent from lock"))?;
        let base = Path::new(text(package, "manifest_path")?)
            .parent()
            .ok_or(Error::Invalid("package path"))?;
        let registry_source = base.parent().ok_or(Error::Invalid("registry directory"))?;
        let registry = registry_source
            .parent()
            .and_then(Path::parent)
            .ok_or(Error::Invalid("registry root"))?;
        let archive = registry
            .join("cache")
            .join(
                registry_source
                    .file_name()
                    .ok_or(Error::Invalid("registry name"))?,
            )
            .join(format!("{name}-{version}.crate"));
        let meta = regular(&archive)?;
        if !meta.is_file() || meta.len() > 128 * 1024 * 1024 {
            return Err(Error::Invalid("registry archive bound"));
        }
        if hash_file(&archive)?.1 != *expected {
            return Err(Error::Message(format!(
                "registry archive checksum mismatch: {name} {version}"
            )));
        }
        package["attow_archive_sha256"] = json!(expected);
        package["attow_archive_path"] = json!(archive);
    }
    Ok(())
}

fn member_hash(archive: &Path, member: &str) -> Result<String> {
    safe_relative(member)?;
    let mut child = Command::new("tar")
        .args(["-xOf"])
        .arg(archive)
        .arg(member)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or(Error::Invalid("tar output pipe"))?
        .take(MAX_NOTICE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_NOTICE {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::Invalid("archive notice member bound"));
    }
    if !child.wait()?.success() {
        return Err(Error::Tool(
            "registry notice member extraction failed".into(),
        ));
    }
    Ok(super::manifest::hash_bytes(&bytes))
}

fn metadata(root: &Path, target: Option<&str>) -> Result<Value> {
    let mut command = Command::new("cargo");
    command.current_dir(root).args([
        "+1.95.0",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--offline",
        "--all-features",
    ]);
    if let Some(target) = target {
        command.args(["--filter-platform", target]);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or(Error::Invalid("missing cargo metadata pipe"))?
        .take(MAX_METADATA + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_METADATA {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::Invalid("metadata byte limit"));
    }
    if !child.wait()?.success() {
        return Err(Error::Tool("locked offline cargo metadata failed".into()));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .ok_or(Error::Invalid("invalid inventory field"))
}
fn array<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or(Error::Invalid("invalid inventory array"))
}
fn read_json(path: &Path) -> Result<Value> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_METADATA {
        return Err(Error::Invalid("inventory JSON limit"));
    }
    Ok(serde_json::from_reader(file)?)
}
fn notice_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|x| x.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    if ["rs", "c", "h", "a", "lib"]
        .contains(&path.extension().and_then(|x| x.to_str()).unwrap_or(""))
    {
        return false;
    }
    [
        "license",
        "licence",
        "copying",
        "copyright",
        "notice",
        "authors",
        "unlicense",
    ]
    .iter()
    .any(|prefix| {
        lower
            .strip_prefix(prefix)
            .is_some_and(|tail| tail.is_empty() || tail.starts_with(['.', '-', '_']))
    })
}
fn regular(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::Invalid("notice symlink"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::Invalid("notice reparse point"));
        }
    }
    Ok(metadata)
}
fn notice_files(base: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![(base.to_owned(), 0usize)];
    let mut found = Vec::new();
    let mut visited = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 32 {
            return Err(Error::Invalid("notice directory depth"));
        }
        regular(&directory)?;
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            visited += 1;
            if visited > MAX_FILES {
                return Err(Error::Invalid("notice scan file limit"));
            }
            let metadata = regular(&path)?;
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else if metadata.is_file() && notice_name(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}
fn relative(path: &Path, base: &Path) -> Result<String> {
    let value = path
        .strip_prefix(base)
        .map_err(|_| Error::Invalid("notice escapes root"))?
        .to_str()
        .ok_or(Error::Invalid("non UTF-8 notice path"))?
        .replace('\\', "/");
    safe_relative(&value)?;
    Ok(value)
}
struct Writer<'a> {
    output: &'a Path,
    artifacts: BTreeSet<String>,
    folded: BTreeSet<String>,
    bytes: u64,
}
impl Writer<'_> {
    fn copy(&mut self, source: &Path, destination: &str, expected: Option<&str>) -> Result<Value> {
        safe_relative(destination)?;
        for ancestor in source.ancestors() {
            regular(ancestor)?;
        }
        let metadata = regular(source)?;
        if !metadata.is_file() || metadata.len() > MAX_NOTICE {
            return Err(Error::Invalid("notice file bound"));
        }
        let (size, sha256) = hash_file(source)?;
        if expected.is_some_and(|x| x != sha256) {
            return Err(Error::Invalid("notice digest mismatch"));
        }
        if self.artifacts.contains(destination) {
            return Ok(json!({"path":destination,"bytes":size,"sha256":sha256}));
        }
        self.bytes = self
            .bytes
            .checked_add(size)
            .ok_or(Error::Invalid("notice size overflow"))?;
        if self.bytes > MAX_TOTAL
            || self.artifacts.len() >= MAX_FILES
            || !self.folded.insert(destination.to_ascii_lowercase())
        {
            return Err(Error::Invalid("notice inventory bound or collision"));
        }
        let target = self.output.join(destination);
        fs::create_dir_all(target.parent().ok_or(Error::Invalid("notice parent"))?)?;
        for ancestor in target
            .parent()
            .ok_or(Error::Invalid("notice parent"))?
            .ancestors()
        {
            regular(ancestor)?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        let mut input = fs::File::open(source)?;
        let copied = std::io::copy(&mut (&mut input).take(MAX_NOTICE + 1), &mut file)?;
        if copied != size {
            return Err(Error::Invalid("notice changed while copying"));
        }
        file.flush()?;
        if hash_file(&self.output.join(destination))?.1 != sha256 {
            return Err(Error::Invalid("notice changed while copying"));
        }
        self.artifacts.insert(destination.into());
        Ok(json!({"path":destination,"bytes":size,"sha256":sha256}))
    }
    fn json(&mut self, destination: &str, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        if bytes.len() as u64 > MAX_METADATA {
            return Err(Error::Invalid("inventory output bound"));
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.output.join(destination))?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        self.artifacts.insert(destination.into());
        Ok(())
    }
}

fn collect(
    root: &Path,
    target: &str,
    all: &Value,
    filtered: &Value,
    output: &Path,
) -> Result<InventoryOutput> {
    let packages = array(all, "packages")?;
    if packages.len() > MAX_PACKAGES {
        return Err(Error::Invalid("package count bound"));
    }
    let target_nodes = array(&filtered["resolve"], "nodes")?;
    let target_ids: BTreeSet<&str> = target_nodes
        .iter()
        .map(|x| text(x, "id"))
        .collect::<Result<_>>()?;
    let fallback_root = root.join("docs/notices/stage10");
    let fallback = read_json(&fallback_root.join("fallbacks.json"))?;
    if fallback["schema"] != 1 {
        return Err(Error::Invalid("fallback schema"));
    }
    let fallbacks = array(&fallback, "packages")?;
    fs::create_dir_all(output)?;
    for ancestor in output.ancestors() {
        regular(ancestor)?;
    }
    let mut writer = Writer {
        output,
        artifacts: BTreeSet::new(),
        folded: BTreeSet::new(),
        bytes: 0,
    };
    let mut inventory = Vec::new();
    let mut components = Vec::new();
    let mut references = BTreeMap::new();
    for package in packages {
        let name = text(package, "name")?;
        let version = text(package, "version")?;
        let id = text(package, "id")?;
        let purl = format!("pkg:cargo/{name}@{version}");
        safe_relative(&format!("{name}-{version}"))?;
        if references.insert(id, purl.clone()).is_some() {
            return Err(Error::Invalid("duplicate package identity"));
        }
        let license = text(package, "license")?;
        let target_member = target_ids.contains(id);
        let mut notices = Vec::new();
        let mut checksum = None;
        let mut recovery = Value::Null;
        if !package["source"].is_null() {
            if text(package, "source")? != "registry+https://github.com/rust-lang/crates.io-index" {
                return Err(Error::Invalid("unreviewed registry source"));
            }
            let manifest = Path::new(text(package, "manifest_path")?);
            let base = manifest
                .parent()
                .ok_or(Error::Invalid("package manifest parent"))?;
            let archive = text(package, "attow_archive_sha256")?;
            if archive.len() != 64 || !archive.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::Invalid("invalid package checksum"));
            }
            checksum = Some(archive.to_owned());
            for file in notice_files(base)? {
                let rel = relative(&file, base)?;
                let expected = package
                    .get("attow_archive_path")
                    .and_then(Value::as_str)
                    .map(|archive| {
                        member_hash(Path::new(archive), &format!("{name}-{version}/{rel}"))
                    })
                    .transpose()?;
                notices.push(writer.copy(
                    &file,
                    &format!("notices/{name}-{version}/{rel}"),
                    expected.as_deref(),
                )?);
            }
            let matches: Vec<_> = fallbacks
                .iter()
                .filter(|f| f["name"] == name && f["version"] == version)
                .collect();
            if matches.len() > 1 {
                return Err(Error::Invalid("duplicate fallback package"));
            }
            if let Some(entry) = matches.first() {
                recovery = (*entry).clone();
                for file in array(entry, "files")? {
                    let path = text(file, "path")?;
                    safe_relative(path)?;
                    let copied = writer.copy(
                        &fallback_root.join(path),
                        &format!("notices/upstream/{path}"),
                        Some(text(file, "sha256")?),
                    )?;
                    notices.push(copied);
                }
            }
            if notices.is_empty() {
                return Err(Error::Message(format!(
                    "missing upstream notice: {name} {version}"
                )));
            }
        } else {
            for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
                notices.push(writer.copy(
                    &root.join(name),
                    &format!("notices/project/{name}"),
                    None,
                )?);
            }
        }
        inventory.push(json!({"name":name,"version":version,"purl":purl,"license_expression":license,"source":package["source"],"archive_sha256":checksum,"repository":package["repository"],"target_resolved":target_member,"notices":notices,"upstream_recovery":recovery}));
        if target_member {
            let mut component = json!({"type":"library","bom-ref":purl,"name":name,"version":version,"purl":purl,"licenses":[{"expression":license.replace('/'," OR ").split_whitespace().collect::<Vec<_>>().join(" ")} ]});
            if let Some(checksum) = checksum {
                component["externalReferences"] = json!([{"type":"distribution","url":format!("https://crates.io/api/v1/crates/{name}/{version}/download"),"hashes":[{"alg":"SHA-256","content":checksum}]}]);
            }
            components.push(component);
        }
    }
    let mut dependencies = Vec::new();
    for node in target_nodes {
        let id = text(node, "id")?;
        let reference = references
            .get(id)
            .ok_or(Error::Invalid("unresolved metadata node"))?;
        let mut depends = BTreeSet::new();
        for dependency in array(node, "dependencies")? {
            let id = dependency
                .as_str()
                .ok_or(Error::Invalid("dependency identity"))?;
            if !target_ids.contains(id) {
                return Err(Error::Invalid("dependency outside target metadata"));
            }
            depends.insert(
                references
                    .get(id)
                    .ok_or(Error::Invalid("missing dependency package"))?,
            );
        }
        dependencies.push(json!({"ref":reference,"dependsOn":depends}));
    }
    let scope = "Conservative workspace all-features Cargo metadata filtered to the named target, including development/build dependencies; this is not binary reachability or evidence that another target was built.";
    writer.json("sbom.cdx.json",&json!({"bomFormat":"CycloneDX","specVersion":"1.6","version":1,"metadata":{"tools":{"components":[{"type":"application","name":"Attow Network xtask inventory","version":"0.1.0"}]},"properties":[{"name":"attow:target","value":target},{"name":"attow:scope","value":scope}]},"components":components,"dependencies":dependencies}))?;
    writer.json("dependency-inventory.json",&json!({"schema":1,"project":"Attow Network","target":target,"scope":"All-source Cargo workspace resolution, including target-specific, development and build packages; notices conservatively include inactive source subcomponents.","target_scope":scope,"notice_tool":all["attow_notice_tool"],"packages":inventory}))?;
    Ok(InventoryOutput {
        artifacts: writer.artifacts.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    #[ignore = "requires the complete locked registry cache and native tar; run explicitly for release evidence"]
    fn real_locked_inventory() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("valid inventory test fixture");
        let output = tempfile::tempdir().expect("valid inventory test fixture");
        let result = generate(root, "x86_64-pc-windows-msvc", output.path())
            .expect("valid inventory test fixture");
        let inventory = read_json(&output.path().join("dependency-inventory.json"))
            .expect("valid inventory test fixture");
        let bom =
            read_json(&output.path().join("sbom.cdx.json")).expect("valid inventory test fixture");
        assert!(
            array(&inventory, "packages")
                .expect("valid inventory test fixture")
                .len()
                > 100
        );
        assert!(
            array(&bom, "components")
                .expect("valid inventory test fixture")
                .len()
                > 100
        );
        assert!(result.artifacts.len() > 400);
        println!(
            "inventory packages={} target components={} generated files={}",
            array(&inventory, "packages")
                .expect("valid inventory test fixture")
                .len(),
            array(&bom, "components")
                .expect("valid inventory test fixture")
                .len(),
            result.artifacts.len()
        );
    }

    fn fixture(root: &Path) -> Value {
        for name in ["alpha", "beta"] {
            let dir = root.join(name);
            fs::create_dir_all(dir.join("nested")).expect("valid inventory test fixture");
            fs::write(dir.join("Cargo.toml"), "[package]\n").expect("valid inventory test fixture");
            fs::write(dir.join("LICENSE"), format!("{name} exact license\n"))
                .expect("valid inventory test fixture");
            fs::write(dir.join("nested/NOTICE"), "nested attribution\n")
                .expect("valid inventory test fixture");
            fs::write(
                dir.join(".cargo-checksum.json"),
                json!({"package":"a".repeat(64),"files":{}}).to_string(),
            )
            .expect("valid inventory test fixture");
        }
        fs::create_dir_all(root.join("docs/notices/stage10"))
            .expect("valid inventory test fixture");
        fs::write(
            root.join("docs/notices/stage10/fallbacks.json"),
            "{\"schema\":1,\"packages\":[]}",
        )
        .expect("valid inventory test fixture");
        json!({"packages":[
            {"id":"alpha-id","name":"alpha","version":"1.0.0","license":"MIT OR Apache-2.0","source":"registry+https://github.com/rust-lang/crates.io-index","manifest_path":root.join("alpha/Cargo.toml"),"repository":null,"attow_archive_sha256":"a".repeat(64)},
            {"id":"beta-id","name":"beta","version":"1.0.0","license":"BSD-3-Clause","source":"registry+https://github.com/rust-lang/crates.io-index","manifest_path":root.join("beta/Cargo.toml"),"repository":null,"attow_archive_sha256":"a".repeat(64)}
        ],"resolve":{"nodes":[{"id":"alpha-id","dependencies":[]},{"id":"beta-id","dependencies":[]}]}})
    }

    #[test]
    fn target_scope_and_nested_attribution_are_preserved() {
        let temp = tempfile::tempdir().expect("valid inventory test fixture");
        let all = fixture(temp.path());
        let target = json!({"resolve":{"nodes":[{"id":"alpha-id","dependencies":[]}]}});
        let out = temp.path().join("out");
        let generated = collect(temp.path(), "x86_64-pc-windows-msvc", &all, &target, &out)
            .expect("valid inventory test fixture");
        let bom: Value = serde_json::from_slice(
            &fs::read(out.join("sbom.cdx.json")).expect("valid inventory test fixture"),
        )
        .expect("valid inventory test fixture");
        assert_eq!(
            bom["components"]
                .as_array()
                .expect("valid inventory test fixture")
                .len(),
            1
        );
        assert_eq!(bom["components"][0]["name"], "alpha");
        assert_eq!(
            fs::read(out.join("notices/beta-1.0.0/nested/NOTICE"))
                .expect("valid inventory test fixture"),
            b"nested attribution\n"
        );
        assert!(
            generated
                .artifacts
                .contains(&"dependency-inventory.json".into())
        );
    }

    #[test]
    fn absent_upstream_notice_fails_closed() {
        let temp = tempfile::tempdir().expect("valid inventory test fixture");
        let all = fixture(temp.path());
        fs::remove_file(temp.path().join("alpha/LICENSE")).expect("valid inventory test fixture");
        fs::remove_file(temp.path().join("alpha/nested/NOTICE"))
            .expect("valid inventory test fixture");
        assert!(
            collect(
                temp.path(),
                "x86_64-pc-windows-msvc",
                &all,
                &all,
                &temp.path().join("out")
            )
            .is_err()
        );
    }

    #[test]
    fn fallback_digest_is_checked_before_copy() {
        let temp = tempfile::tempdir().expect("valid inventory test fixture");
        let all = fixture(temp.path());
        let fallback = temp.path().join("docs/notices/stage10");
        fs::write(fallback.join("LICENSE"), "tampered").expect("valid inventory test fixture");
        fs::write(fallback.join("fallbacks.json"),json!({"schema":1,"packages":[{"name":"alpha","version":"1.0.0","files":[{"path":"LICENSE","sha256":"0".repeat(64),"source":"https://example.invalid/pinned/LICENSE"}]}]}).to_string()).expect("valid inventory test fixture");
        assert!(
            collect(
                temp.path(),
                "x86_64-pc-windows-msvc",
                &all,
                &all,
                &temp.path().join("out")
            )
            .is_err()
        );
    }
}
