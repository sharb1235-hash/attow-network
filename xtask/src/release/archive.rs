use super::{
    ReleaseError, Result,
    manifest::{self, safe_relative},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 8192;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
fn register(names: &mut BTreeSet<String>, path: &str) -> Result<()> {
    safe_relative(path)?;
    let lower = path.to_ascii_lowercase();
    if names.len() >= MAX_ENTRIES
        || names.iter().any(|n| {
            n == &lower
                || n.starts_with(&(lower.clone() + "/"))
                || lower.starts_with(&(n.clone() + "/"))
        })
    {
        return Err(ReleaseError::Invalid(
            "duplicate or overlapping archive path",
        ));
    }
    names.insert(lower);
    Ok(())
}
fn octal(bytes: &[u8]) -> Result<u64> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ReleaseError::Invalid("tar number"))?
        .trim_matches(['\0', ' ']);
    if text.is_empty() {
        return Ok(0);
    }
    if !text.bytes().all(|b| matches!(b, b'0'..=b'7')) {
        return Err(ReleaseError::Invalid("tar number"));
    }
    u64::from_str_radix(text, 8).map_err(|_| ReleaseError::Invalid("tar number overflow"))
}
fn field(bytes: &[u8]) -> Result<String> {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    if bytes[end..].iter().any(|b| *b != 0) {
        return Err(ReleaseError::Invalid("tar string padding"));
    }
    String::from_utf8(bytes[..end].to_vec()).map_err(|_| ReleaseError::Invalid("tar string"))
}
fn copy_hash(
    reader: &mut impl Read,
    mut size: u64,
    mut writer: Option<&mut fs::File>,
) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    while size > 0 {
        let n = usize::try_from(size.min(buffer.len() as u64))
            .map_err(|_| ReleaseError::Invalid("archive size"))?;
        reader.read_exact(&mut buffer[..n])?;
        hash.update(&buffer[..n]);
        if let Some(w) = writer.as_mut() {
            w.write_all(&buffer[..n])?;
        }
        size -= n as u64;
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn parse_tar(path: &Path, destination: Option<&Path>) -> Result<Vec<ArchiveEntry>> {
    manifest::reject_link(path)?;
    if fs::metadata(path)?.len() > MAX_TOTAL + MAX_ENTRIES as u64 * 1024 {
        return Err(ReleaseError::Invalid("archive size exceeds limit"));
    }
    let mut reader = fs::File::open(path)?;
    let mut entries = Vec::new();
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    loop {
        let mut h = [0; 512];
        reader.read_exact(&mut h)?;
        if h.iter().all(|b| *b == 0) {
            let mut end = [0; 512];
            reader.read_exact(&mut end)?;
            let mut trailing = [0; 1];
            if end.iter().any(|b| *b != 0) || reader.read(&mut trailing)? != 0 || entries.is_empty()
            {
                return Err(ReleaseError::Invalid("invalid tar ending"));
            }
            break;
        }
        let expected = octal(&h[148..156])?;
        let sum: u64 = h
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(*b)
                }
            })
            .sum();
        if expected != sum
            || &h[257..263] != b"ustar\0"
            || &h[263..265] != b"00"
            || !matches!(h[156], 0 | b'0')
            || h[157..257].iter().any(|b| *b != 0)
        {
            return Err(ReleaseError::Invalid("unsupported or corrupt tar header"));
        }
        let name = field(&h[..100])?;
        let prefix = field(&h[345..500])?;
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        register(&mut names, &path)?;
        let size = octal(&h[124..136])?;
        total = total
            .checked_add(size)
            .ok_or(ReleaseError::Invalid("archive aggregate overflow"))?;
        if size > MAX_FILE || total > MAX_TOTAL {
            return Err(ReleaseError::Invalid("archive content limit"));
        }
        let mut file = if let Some(dir) = destination {
            let p = dir.join(&path);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent)?;
            }
            Some(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(p)?,
            )
        } else {
            None
        };
        let hash = copy_hash(&mut reader, size, file.as_mut())?;
        let padding = ((512 - size % 512) % 512) as usize;
        let mut pad = [0; 512];
        reader.read_exact(&mut pad[..padding])?;
        if pad[..padding].iter().any(|b| *b != 0) {
            return Err(ReleaseError::Invalid("nonzero tar padding"));
        }
        entries.push(ArchiveEntry {
            path,
            bytes: size,
            sha256: hash,
        });
    }
    Ok(entries)
}
pub fn validate_tar(path: &Path) -> Result<Vec<ArchiveEntry>> {
    parse_tar(path, None)
}
/// Only producer-owned, already verified archives are extracted to a new private directory.
pub fn extract_new(path: &Path, destination: &Path) -> Result<Vec<ArchiveEntry>> {
    validate_tar(path)?;
    fs::create_dir(destination)?;
    manifest::reject_link(destination)?;
    parse_tar(path, Some(destination))
}
fn header(path: &str, size: u64) -> Result<[u8; 512]> {
    safe_relative(path)?;
    let mut h = [0; 512];
    let (prefix, name) = if path.len() <= 100 {
        ("", path)
    } else {
        let cut = path
            .char_indices()
            .filter(|(i, c)| *c == '/' && *i <= 155 && path.len() - i - 1 <= 100)
            .map(|(i, _)| i)
            .next_back()
            .ok_or(ReleaseError::Invalid("tar path length"))?;
        (&path[..cut], &path[cut + 1..])
    };
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    h[100..108].copy_from_slice(b"0000755\0");
    h[108..116].copy_from_slice(b"0000000\0");
    h[116..124].copy_from_slice(b"0000000\0");
    h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    h[136..148].copy_from_slice(b"00000000000\0");
    h[148..156].fill(b' ');
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let sum: u64 = h.iter().map(|b| u64::from(*b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    Ok(h)
}
struct TarWriter {
    file: fs::File,
    names: BTreeSet<String>,
    total: u64,
}
impl TarWriter {
    fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            file: fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?,
            names: BTreeSet::new(),
            total: 0,
        })
    }
    fn append(&mut self, path: &str, size: u64, reader: &mut impl Read) -> Result<()> {
        register(&mut self.names, path)?;
        self.total = self
            .total
            .checked_add(size)
            .ok_or(ReleaseError::Invalid("archive size overflow"))?;
        if size > MAX_FILE || self.total > MAX_TOTAL {
            return Err(ReleaseError::Invalid("archive content limit"));
        }
        self.file.write_all(&header(path, size)?)?;
        copy_hash(reader, size, Some(&mut self.file))?;
        self.file
            .write_all(&[0; 512][..((512 - size % 512) % 512) as usize])?;
        Ok(())
    }
    fn finish(mut self) -> Result<()> {
        if self.names.is_empty() {
            return Err(ReleaseError::Invalid("empty archive"));
        }
        self.file.write_all(&[0; 1024])?;
        self.file.sync_all()?;
        Ok(())
    }
}
pub fn write_files(path: &Path, files: &[(String, PathBuf)]) -> Result<()> {
    let mut files = files.to_vec();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut writer = TarWriter::new(path)?;
    for (name, source) in files {
        manifest::reject_link(&source)?;
        let mut file = fs::File::open(source)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(ReleaseError::Invalid("archive input not regular"));
        }
        writer.append(&name, meta.len(), &mut file)?;
    }
    writer.finish()
}
pub fn allowed_source(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    safe_relative(path).is_ok()
        && !p.split('/').any(|c| {
            matches!(
                c,
                ".git"
                    | ".attow"
                    | ".tools"
                    | "target"
                    | "node_modules"
                    | ".browsers"
                    | "release-candidate"
                    | "credentials"
            ) || c == ".env"
                || c.starts_with(".env.")
        })
        && !p.ends_with(".pem")
        && !p.ends_with(".key")
        && !p.ends_with(".pfx")
        && !p.ends_with(".p12")
}
/// Read committed Git blobs, never checkout transformations or untracked local state.
pub fn source_archive(root: &Path, output: &Path) -> Result<()> {
    let tree = Command::new("git")
        .args(["ls-tree", "-r", "-z", "HEAD"])
        .current_dir(root)
        .output()?;
    if !tree.status.success() || tree.stdout.len() > 8 * 1024 * 1024 {
        return Err(ReleaseError::Tool(
            "git source tree failed or exceeds limit".into(),
        ));
    }
    let mut files = Vec::new();
    for row in tree.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let row =
            std::str::from_utf8(row).map_err(|_| ReleaseError::Invalid("source path encoding"))?;
        let (meta, path) = row
            .split_once('\t')
            .ok_or(ReleaseError::Invalid("git tree format"))?;
        if !allowed_source(path) {
            continue;
        }
        let bits: Vec<_> = meta.split(' ').collect();
        if bits.len() != 3 || !matches!(bits[0], "100644" | "100755") || bits[1] != "blob" {
            return Err(ReleaseError::Invalid("source contains a link or submodule"));
        }
        files.push((path.to_owned(), bits[2].to_owned()));
    }
    files.sort();
    let mut writer = TarWriter::new(output)?;
    let mut child = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let result = (|| -> Result<()> {
        let mut input = child
            .stdin
            .take()
            .ok_or(ReleaseError::Invalid("git stdin"))?;
        let mut reader = BufReader::new(
            child
                .stdout
                .take()
                .ok_or(ReleaseError::Invalid("git stdout"))?,
        );
        for (path, id) in files {
            writeln!(input, "{id}")?;
            input.flush()?;
            let mut line = String::new();
            reader.read_line(&mut line)?;
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() != 3 || parts[0] != id || parts[1] != "blob" {
                return Err(ReleaseError::Invalid("git blob response"));
            }
            let size = parts[2]
                .parse()
                .map_err(|_| ReleaseError::Invalid("git blob size"))?;
            writer.append(&path, size, &mut reader)?;
            let mut newline = [0];
            reader.read_exact(&mut newline)?;
            if newline != [b'\n'] {
                return Err(ReleaseError::Invalid("git blob delimiter"));
            }
        }
        drop(input);
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    result?;
    if !status.success() {
        return Err(ReleaseError::Tool("git blob export failed".into()));
    }
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header(path: &str, kind: u8, size: u64) -> [u8; 512] {
        let mut h = [0; 512];
        h[..path.len()].copy_from_slice(path.as_bytes());
        h[100..108].copy_from_slice(b"0000644\0");
        h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
        h[148..156].fill(b' ');
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        let sum: u64 = h.iter().map(|v| u64::from(*v)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        h
    }
    fn tar(headers: &[(&str, u8, u64)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (p, k, n) in headers {
            bytes.extend(header(p, *k, *n));
            if *n < 1024 {
                bytes.extend(vec![0; ((*n).div_ceil(512) * 512) as usize]);
            }
        }
        bytes.extend([0; 1024]);
        bytes
    }
    #[test]
    fn valid_regular_archive_is_hashed_without_execution() -> Result<()> {
        let t = tempfile::tempdir()?;
        let p = t.path().join("test.tar");
        std::fs::write(&p, tar(&[("bin/attow.exe", b'0', 3)]))?;
        let entries = validate_tar(&p)?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].bytes, 3);
        assert_eq!(
            entries[0].sha256,
            super::super::manifest::hash_bytes(&[0, 0, 0])
        );
        Ok(())
    }
    #[test]
    fn hostile_archive_members_are_rejected() -> Result<()> {
        let t = tempfile::tempdir()?;
        let p = t.path().join("test.tar");
        for entries in [
            vec![("../outside", b'0', 0)],
            vec![("bin/link", b'2', 0)],
            vec![("a", b'0', 0), ("A", b'0', 0)],
            vec![("a", b'0', 0), ("a/b", b'0', 0)],
            vec![("too-big", b'0', MAX_FILE + 1)],
        ] {
            std::fs::write(&p, tar(&entries))?;
            assert!(validate_tar(&p).is_err());
        }
        let mut altered = tar(&[("ok", b'0', 1)]);
        altered[0] = b'z';
        std::fs::write(&p, altered)?;
        assert!(validate_tar(&p).is_err());
        Ok(())
    }
    #[test]
    fn source_archive_exports_committed_bytes_without_local_credentials_or_caches() -> Result<()> {
        let t = tempfile::tempdir()?;
        let root = t.path().join("mini repository");
        fs::create_dir(&root)?;
        for (path, bytes) in [
            ("src/lib.rs", b"committed\n".as_slice()),
            (".env", b"synthetic secret"),
            ("target/cache", b"cache"),
            ("credentials/token", b"synthetic token"),
            ("signing.key", b"synthetic key"),
        ] {
            let file = root.join(path);
            fs::create_dir_all(file.parent().expect("parent"))?;
            fs::write(file, bytes)?;
        }
        for args in [
            vec!["init", "--quiet"],
            vec!["add", "--all"],
            vec![
                "-c",
                "user.name=Archive Test",
                "-c",
                "user.email=archive-test@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&root)
                    .output()?
                    .status
                    .success()
            );
        }
        fs::write(root.join("src/lib.rs"), "changed working copy\r\n")?;
        let path = t.path().join("source.tar");
        source_archive(&root, &path)?;
        let entries = validate_tar(&path)?;
        assert_eq!(
            entries,
            vec![ArchiveEntry {
                path: "src/lib.rs".into(),
                bytes: 10,
                sha256: manifest::hash_bytes(b"committed\n")
            }]
        );
        let extracted = t.path().join("new folder");
        extract_new(&path, &extracted)?;
        assert_eq!(fs::read(extracted.join("src/lib.rs"))?, b"committed\n");
        assert!(extract_new(&path, &extracted).is_err());
        Ok(())
    }
    #[test]
    fn traversal_is_rejected_before_creating_install_directory() -> Result<()> {
        let t = tempfile::tempdir()?;
        let path = t.path().join("bad.tar");
        fs::write(&path, tar(&[("../escaped", b'0', 0)]))?;
        let destination = t.path().join("installation");
        assert!(extract_new(&path, &destination).is_err());
        assert!(!destination.exists());
        assert!(!t.path().join("escaped").exists());
        Ok(())
    }
}
