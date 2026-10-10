//! Repository policy checks for Attow Network.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_TEXT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug)]
struct CheckError {
    path: PathBuf,
    message: String,
}

impl fmt::Display for CheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.message)
    }
}

impl Error for CheckError {}

fn rejected(path: &Path, message: impl Into<String>) -> Box<dyn Error> {
    Box::new(CheckError {
        path: path.to_owned(),
        message: message.into(),
    })
}

#[derive(Debug, Default)]
struct Table {
    name: Vec<String>,
    values: BTreeMap<String, String>,
}

fn read_text(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut value = String::new();
    File::open(path)?
        .take(MAX_TEXT_BYTES + 1)
        .read_to_string(&mut value)?;
    if value.len() as u64 > MAX_TEXT_BYTES {
        return Err(rejected(path, "text exceeds repository checker size limit"));
    }
    Ok(value)
}

// Cargo remains the TOML syntax authority. This scanner reads the policy-relevant
// manifest tables without adding a parser dependency to the foundation stage.
fn split_unquoted(value: &str, delimiter: char) -> Vec<&str> {
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_u32;
    let mut start = 0;
    let mut parts = Vec::new();
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(current) = quote {
            if current == character {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth = depth.saturating_sub(1),
            _ if character == delimiter && (depth == 0 || delimiter == '#') => {
                parts.push(value[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches(['"', '\''])
}

fn assignment(value: &str) -> Option<(String, String)> {
    let pieces = split_unquoted(value, '=');
    if pieces.len() < 2 {
        return None;
    }
    let key = pieces[0];
    Some((
        unquote(key).to_owned(),
        value[key.len()..]
            .trim_start()
            .strip_prefix('=')?
            .trim()
            .to_owned(),
    ))
}

fn parse_manifest(path: &Path) -> Result<Vec<Table>, Box<dyn Error>> {
    let text = read_text(path)?;
    let mut tables = vec![Table::default()];
    let mut pending = String::new();
    for raw in text.lines() {
        let pieces = split_unquoted(raw, '#');
        let line = pieces[0].trim();
        if line.is_empty() {
            continue;
        }
        if pending.is_empty() && line.starts_with('[') && line.ends_with(']') {
            let name = split_unquoted(line.trim_matches(['[', ']']), '.')
                .into_iter()
                .map(|part| unquote(part).to_owned())
                .collect();
            tables.push(Table {
                name,
                values: BTreeMap::new(),
            });
            continue;
        }
        pending.push_str(line);
        pending.push(' ');
        let brackets = balanced(&pending);
        if !brackets {
            continue;
        }
        let (key, value) = assignment(pending.trim())
            .ok_or_else(|| rejected(path, "unsupported manifest assignment"))?;
        let table = tables
            .last_mut()
            .ok_or_else(|| rejected(path, "missing manifest table"))?;
        table.values.insert(key, value);
        pending.clear();
    }
    if !pending.is_empty() {
        return Err(rejected(path, "unterminated manifest value"));
    }
    Ok(tables)
}

fn balanced(value: &str) -> bool {
    let mut depth = 0_i32;
    let mut quote = None;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(current) = quote {
            if character == current {
                quote = None;
            }
        } else {
            match character {
                '\'' | '"' => quote = Some(character),
                '[' | '{' => depth += 1,
                ']' | '}' => depth -= 1,
                _ => {}
            }
        }
    }
    depth == 0 && quote.is_none()
}

fn table<'a>(tables: &'a [Table], name: &[&str]) -> Option<&'a Table> {
    tables.iter().find(|entry| {
        entry
            .name
            .iter()
            .map(String::as_str)
            .eq(name.iter().copied())
    })
}

fn inline_values(value: &str) -> BTreeMap<String, String> {
    split_unquoted(
        value.trim().trim_start_matches('{').trim_end_matches('}'),
        ',',
    )
    .into_iter()
    .filter_map(assignment)
    .collect()
}

fn exact_version(version: &str) -> bool {
    let Some(version) = unquote(version).strip_prefix('=') else {
        return false;
    };
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let numbers: Vec<_> = core.split('.').collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
        && !version
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '*' | '^' | '~' | ',' | '>' | '<' | '|'))
}

fn verify_dependency(
    path: &Path,
    name: &str,
    value: &str,
    inherited: &BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let fields = inline_values(value);
    if fields.get("workspace").is_some_and(|value| value == "true") {
        let source = inherited.get(name).ok_or_else(|| {
            rejected(
                path,
                format!("dependency {name} has no workspace definition"),
            )
        })?;
        if inline_values(source).contains_key("workspace") {
            return Err(rejected(
                path,
                format!("dependency {name} has recursive workspace inheritance"),
            ));
        }
        // Workspace entries, including their paths relative to the workspace
        // manifest, have already been checked before member manifests.
        return Ok(());
    }
    let version = fields.get("version").map(String::as_str).unwrap_or(value);
    if !exact_version(version) || fields.contains_key("git") {
        return Err(rejected(
            path,
            format!(
                "dependency {name} must use an exact version (=major.minor.patch), without a git source"
            ),
        ));
    }
    if let Some(location) = fields.get("path") {
        let parent = path
            .parent()
            .ok_or_else(|| rejected(path, "manifest has no parent"))?;
        if !parent.join(unquote(location)).join("Cargo.toml").is_file() {
            return Err(rejected(
                path,
                format!(
                    "dependency {name} has a missing path manifest: {}",
                    unquote(location)
                ),
            ));
        }
    }
    Ok(())
}

fn dependencies(tables: &[Table], workspace: bool) -> Vec<(String, String)> {
    let mut output = Vec::new();
    for entry in tables {
        if entry.name.first().is_some_and(|name| name == "workspace") != workspace {
            continue;
        }
        let Some(position) = entry.name.iter().position(|name| {
            matches!(
                name.as_str(),
                "dependencies" | "dev-dependencies" | "build-dependencies"
            )
        }) else {
            continue;
        };
        if position + 1 == entry.name.len() {
            for (key, value) in &entry.values {
                if let Some(name) = key.strip_suffix(".workspace") {
                    output.push((name.to_owned(), format!("{{ workspace = {value} }}")));
                } else {
                    output.push((key.clone(), value.clone()));
                }
            }
        } else if position + 2 == entry.name.len() {
            let fields = entry
                .values
                .iter()
                .map(|(key, value)| format!("{key} = {value}"))
                .collect::<Vec<_>>()
                .join(", ");
            output.push((entry.name[position + 1].clone(), format!("{{ {fields} }}")));
        }
    }
    output
}

fn verify_manifest(
    path: &Path,
    tables: &[Table],
    workspace: &[Table],
    inherited: &BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let Some(package) = table(tables, &["package"]) else {
        return Ok(());
    };
    let metadata = table(workspace, &["workspace", "package"]);
    for (field, expected) in [("publish", "false"), ("license", "MIT OR Apache-2.0")] {
        let value = if package
            .values
            .get(&format!("{field}.workspace"))
            .is_some_and(|value| value == "true")
        {
            metadata.and_then(|entry| entry.values.get(field))
        } else {
            package.values.get(field)
        };
        if value.map(|value| unquote(value)) != Some(expected) {
            return Err(rejected(
                path,
                format!(
                    "package {field} must be {expected}, directly or through workspace inheritance"
                ),
            ));
        }
    }
    for (name, value) in dependencies(tables, false) {
        verify_dependency(path, &name, &value, inherited)?;
    }
    let parent = path
        .parent()
        .ok_or_else(|| rejected(path, "manifest has no parent"))?;
    let mut declared_target = false;
    for target in tables.iter().filter(|entry| {
        entry.name.len() == 1
            && matches!(
                entry.name[0].as_str(),
                "lib" | "bin" | "example" | "test" | "bench"
            )
    }) {
        declared_target = true;
        let kind = target.name[0].as_str();
        let candidates = if let Some(path) = target.values.get("path") {
            vec![PathBuf::from(unquote(path))]
        } else if kind == "lib" {
            vec![PathBuf::from("src/lib.rs")]
        } else {
            let name = target
                .values
                .get("name")
                .ok_or_else(|| rejected(path, format!("{kind} target requires a name or path")))?;
            let name = unquote(name);
            let directory = match kind {
                "bin" => "src/bin",
                "example" => "examples",
                "test" => "tests",
                _ => "benches",
            };
            let mut candidates = vec![
                PathBuf::from(format!("{directory}/{name}.rs")),
                PathBuf::from(format!("{directory}/{name}/main.rs")),
            ];
            if kind == "bin"
                && package
                    .values
                    .get("name")
                    .is_some_and(|package| unquote(package) == name)
            {
                candidates.push(PathBuf::from("src/main.rs"));
            }
            candidates
        };
        if !candidates
            .iter()
            .any(|candidate| parent.join(candidate).is_file())
        {
            return Err(rejected(
                path,
                format!(
                    "missing manifest target: {}",
                    candidates
                        .iter()
                        .map(|candidate| candidate.display().to_string())
                        .collect::<Vec<_>>()
                        .join(" or ")
                ),
            ));
        }
    }
    if !declared_target
        && !parent.join("src/lib.rs").is_file()
        && !parent.join("src/main.rs").is_file()
        && !parent.join("src/bin").is_dir()
    {
        return Err(rejected(path, "package has no manifest target"));
    }
    Ok(())
}

fn repository_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        let kind = entry.file_type()?;
        if kind.is_symlink()
            || relative.starts_with("docs/chronos")
            || matches!(
                entry.file_name().to_str(),
                Some(".git" | "target" | "node_modules")
            )
        {
            continue;
        }
        if kind.is_dir() {
            repository_files(root, &path, output)?;
        } else if kind.is_file() {
            output.push(path);
        }
        if output.len() > 100_000 {
            return Err(rejected(
                root,
                "repository checker file count limit exceeded",
            ));
        }
    }
    Ok(())
}

fn prose_lines(text: &str) -> Vec<&str> {
    let mut fence = None;
    let mut result = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next();
        let count = trimmed
            .chars()
            .take_while(|character| Some(*character) == marker)
            .count();
        if count >= 3 && matches!(marker, Some('`' | '~')) {
            if let Some((open, width)) = fence {
                if marker == Some(open) && count >= width {
                    fence = None;
                }
            } else if let Some(marker) = marker {
                fence = Some((marker, count));
            }
            continue;
        }
        if fence.is_none() && !line.starts_with("    ") && !line.starts_with('\t') {
            result.push(line);
        }
    }
    result
}

fn anchors(text: &str) -> BTreeSet<String> {
    let mut output = BTreeSet::new();
    let mut counts = BTreeMap::new();
    let lines = prose_lines(text);
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let heading = if trimmed.starts_with('#') {
            Some(
                trimmed
                    .trim_start_matches('#')
                    .trim()
                    .trim_end_matches('#')
                    .trim(),
            )
        } else if index > 0
            && trimmed.len() >= 3
            && (trimmed.chars().all(|c| c == '=') || trimmed.chars().all(|c| c == '-'))
        {
            Some(lines[index - 1].trim())
        } else {
            None
        };
        if let Some(heading) = heading {
            let slug: String = heading
                .to_lowercase()
                .chars()
                .filter_map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        Some(c)
                    } else if c.is_whitespace() {
                        Some('-')
                    } else {
                        None
                    }
                })
                .collect();
            let count = counts.entry(slug.clone()).or_insert(0);
            output.insert(if *count == 0 {
                slug
            } else {
                format!("{slug}-{count}")
            });
            *count += 1;
        }
        for attribute in ["id=\"", "name=\""] {
            for suffix in line.split(attribute).skip(1) {
                if let Some((id, _)) = suffix.split_once('"') {
                    output.insert(id.to_owned());
                }
            }
        }
    }
    output
}

fn link_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for line in prose_lines(text) {
        let trimmed = line.trim();
        if trimmed.starts_with('[')
            && let Some((_, suffix)) = trimmed.split_once("]:")
        {
            targets.push(link_destination(suffix));
        }
        let mut remaining = line;
        while let Some(start) = remaining.find("](") {
            remaining = &remaining[start + 2..];
            let mut depth = 1;
            let mut end = None;
            for (index, character) in remaining.char_indices() {
                match character {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    end = Some(index);
                    break;
                }
            }
            let Some(end) = end else {
                break;
            };
            targets.push(link_destination(&remaining[..end]));
            remaining = &remaining[end + 1..];
        }
    }
    targets
}

fn link_destination(value: &str) -> String {
    let value = value.trim();
    if let Some(value) = value.strip_prefix('<') {
        return value.split('>').next().unwrap_or_default().to_owned();
    }
    value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn decode_url(value: &str) -> Result<String, Box<dyn Error>> {
    let mut bytes = Vec::with_capacity(value.len());
    let raw = value.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            let pair = std::str::from_utf8(&raw[index + 1..index + 3])?;
            bytes.push(u8::from_str_radix(pair, 16)?);
            index += 3;
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    Ok(String::from_utf8(bytes)?)
}

fn verify_markdown(root: &Path, path: &Path) -> Result<(), Box<dyn Error>> {
    let text = read_text(path)?;
    for target in link_targets(&text) {
        if target.is_empty()
            || target.starts_with("//")
            || target
                .split(['/', '#'])
                .next()
                .is_some_and(|prefix| prefix.contains(':'))
        {
            continue;
        }
        let (location, anchor) = target
            .split_once('#')
            .map_or((target.as_str(), None), |(location, anchor)| {
                (location, Some(anchor))
            });
        let location = decode_url(location.split('?').next().unwrap_or_default())?;
        let destination = if location.is_empty() {
            path.to_owned()
        } else if let Some(location) = location.strip_prefix('/') {
            root.join(location)
        } else {
            path.parent()
                .ok_or_else(|| rejected(path, "document has no parent"))?
                .join(location)
        };
        if !destination.exists() {
            return Err(rejected(
                path,
                format!("broken internal documentation link: {target}"),
            ));
        }
        if destination
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            && let Some(anchor) = anchor.filter(|anchor| !anchor.is_empty())
            && !anchors(&read_text(&destination)?).contains(&decode_url(anchor)?)
        {
            return Err(rejected(
                path,
                format!("broken internal documentation anchor: {target}"),
            ));
        }
    }
    Ok(())
}

/// Validate repository manifests, licenses, targets, and local documentation links.
pub fn verify_repository(root: &Path) -> Result<(), Box<dyn Error>> {
    for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
        if !root.join(name).is_file() {
            return Err(rejected(root, format!("missing license file {name}")));
        }
    }
    let manifest = root.join("Cargo.toml");
    let workspace = parse_manifest(&manifest)?;
    let inherited: BTreeMap<_, _> = dependencies(&workspace, true).into_iter().collect();
    for (name, value) in &inherited {
        verify_dependency(&manifest, name, value, &inherited)?;
    }
    let mut files = Vec::new();
    repository_files(root, root, &mut files)?;
    files.sort();
    for path in files {
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            verify_manifest(&path, &parse_manifest(&path)?, &workspace, &inherited)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            verify_markdown(root, &path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::verify_repository;
    use std::{
        error::Error,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Result<Self, Box<dyn Error>> {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("attow-checker-{}-{id}", std::process::id()));
            fs::create_dir(&root)?;
            let fixture = Self(root);
            fixture.write("Cargo.toml", "[workspace]\nmembers = [\"child\"]\n[workspace.package]\nlicense = \"MIT OR Apache-2.0\"\npublish = false\n[workspace.dependencies]\ncodec = { version = \"=1.2.3\" }\n[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nlicense.workspace = true\npublish.workspace = true\n")?;
            fixture.write("src/lib.rs", "")?;
            fixture.write("LICENSE-MIT", "Fixture MIT license")?;
            fixture.write("LICENSE-APACHE", "Fixture Apache license")?;
            fixture.write("README.md", "# Example\n\n[Guide](docs/guide.md#usage)\n")?;
            fixture.write("docs/guide.md", "# Usage\n\n[Home](../README.md)\n")?;
            fixture.write("child/Cargo.toml", "[package]\nname = \"child\"\nversion = \"0.1.0\"\nlicense.workspace = true\npublish.workspace = true\n[dependencies]\ncodec.workspace = true\n")?;
            fixture.write("child/src/lib.rs", "")?;
            Ok(fixture)
        }

        fn write(&self, path: &str, contents: &str) -> Result<(), Box<dyn Error>> {
            let path = self.0.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, contents)?;
            Ok(())
        }

        fn failure(&self, expected: &str) -> Result<(), Box<dyn Error>> {
            match verify_repository(&self.0) {
                Ok(()) => panic!("expected repository check to reject {expected}"),
                Err(error) => assert!(
                    error.to_string().contains(expected),
                    "unexpected error: {error}"
                ),
            }
            Ok(())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn accepts_inherited_metadata_and_pinned_dependencies() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        verify_repository(&fixture.0)
    }

    #[test]
    fn rejects_missing_license() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        fs::remove_file(fixture.0.join("LICENSE-MIT"))?;
        fixture.failure("LICENSE-MIT")
    }

    #[test]
    fn rejects_floating_dependency() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        fixture.write("child/Cargo.toml", "[package]\nname=\"child\"\nversion=\"0.1.0\"\nlicense.workspace=true\npublish.workspace=true\n[dependencies]\nfloating = \"1.2.3\"\n")?;
        fixture.failure("exact version")
    }

    #[test]
    fn rejects_missing_explicit_target() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        let manifest = fs::read_to_string(fixture.0.join("Cargo.toml"))?;
        fixture.write(
            "Cargo.toml",
            &(manifest + "\n[[example]]\nname=\"missing\"\npath=\"examples/missing.rs\"\n"),
        )?;
        fixture.failure("examples/missing.rs")
    }

    #[test]
    fn rejects_enabled_publish() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        let manifest = fs::read_to_string(fixture.0.join("child/Cargo.toml"))?;
        fixture.write(
            "child/Cargo.toml",
            &manifest.replace("publish.workspace = true", "publish = true"),
        )?;
        fixture.failure("publish")
    }

    #[test]
    fn rejects_broken_internal_docs_link() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        fixture.write("README.md", "[Missing](docs/missing.md)\n")?;
        fixture.failure("docs/missing.md")
    }

    #[test]
    fn ignores_fenced_examples_and_original_research() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        fixture.write("README.md", "```markdown\n[Example](missing.md)\n```\n")?;
        fixture.write("docs/chronos/Cargo.toml", "[package]\nname=\"original\"\n")?;
        fixture.write("docs/chronos/README.md", "[Original link](missing.md)\n")?;
        verify_repository(&fixture.0)
    }

    #[test]
    fn rejects_floating_inherited_dependency() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        let manifest = fs::read_to_string(fixture.0.join("Cargo.toml"))?;
        fixture.write("Cargo.toml", &manifest.replace("=1.2.3", "1.2.3"))?;
        fixture.failure("exact version")
    }

    #[test]
    fn rejects_missing_heading_anchor() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        fixture.write("README.md", "[Missing](docs/guide.md#missing)\n")?;
        fixture.failure("missing")
    }

    #[test]
    fn rejects_unpinned_git_dependency() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        let manifest = fs::read_to_string(fixture.0.join("child/Cargo.toml"))?;
        fixture.write(
            "child/Cargo.toml",
            &(manifest
                + "upstream = { git = \"https://example.invalid/upstream\", branch = \"main\" }\n"),
        )?;
        fixture.failure("exact version")
    }

    #[test]
    fn rejects_broken_reference_link() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        fixture.write("README.md", "See [guide][guide].\n\n[guide]: absent.md\n")?;
        fixture.failure("absent.md")
    }

    #[test]
    fn resolves_inherited_paths_at_the_workspace_root() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        let manifest = fs::read_to_string(fixture.0.join("Cargo.toml"))?;
        fixture.write(
            "Cargo.toml",
            &manifest.replace(
                "codec = { version = \"=1.2.3\" }",
                "codec = { version = \"=1.2.3\", path = \"child\" }",
            ),
        )?;
        verify_repository(&fixture.0)
    }

    #[test]
    fn checks_every_target_specific_dependency_declaration() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        let manifest = fs::read_to_string(fixture.0.join("child/Cargo.toml"))?;
        fixture.write("child/Cargo.toml", &(manifest + "\n[target.'cfg(unix)'.dependencies]\ncodec = \"1.2.3\"\n[target.'cfg(windows)'.dependencies]\ncodec = \"=1.2.3\"\n"))?;
        fixture.failure("exact version")
    }
}
