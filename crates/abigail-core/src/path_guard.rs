use anyhow::{anyhow, bail, Context, Result};
use std::path::{Component, Path, PathBuf};

pub fn ensure_expected_filename(path: &Path, expected: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Path '{}' is missing a valid file name", path.display()))?;

    if file_name != expected {
        bail!(
            "Path '{}' must target '{}', found '{}'",
            path.display(),
            expected,
            file_name
        );
    }

    Ok(())
}

pub fn ensure_relative_no_traversal(path: &Path, label: &str) -> Result<()> {
    if path.is_absolute() {
        bail!("{label} must be relative: '{}'", path.display());
    }

    for component in path.components() {
        match component {
            Component::CurDir | Component::Normal(_) => {}
            Component::ParentDir => bail!("{label} must not contain '..': '{}'", path.display()),
            Component::RootDir | Component::Prefix(_) => {
                bail!(
                    "{label} must not contain rooted components: '{}'",
                    path.display()
                )
            }
        }
    }

    Ok(())
}

pub fn resolve_within_root(root: &Path, relative: &Path, label: &str) -> Result<PathBuf> {
    ensure_relative_no_traversal(relative, label)?;
    let candidate = root.join(relative);
    ensure_path_within_root(root, &candidate, label)?;
    Ok(candidate)
}

pub fn ensure_path_within_root(root: &Path, path: &Path, label: &str) -> Result<()> {
    let normalized_root = normalize_path(root);
    let normalized_path = normalize_path(path);

    if !normalized_path.starts_with(&normalized_root) {
        bail!(
            "{label} '{}' escapes root '{}'",
            path.display(),
            root.display()
        );
    }

    Ok(())
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }

    normalized
}

fn ensure_simple_filename(expected: &str) -> Result<()> {
    let expected_path = Path::new(expected);
    let file_name = expected_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Expected file name '{}' is invalid", expected))?;
    if file_name != expected || expected_path.components().count() != 1 {
        bail!(
            "Expected file name '{}' must be a single path segment",
            expected
        );
    }
    Ok(())
}

pub fn trusted_file_path(root: &Path, expected: &str) -> Result<PathBuf> {
    ensure_simple_filename(expected)?;
    resolve_within_root(root, Path::new(expected), expected)
}

pub fn load_string_from_root(root: &Path, expected: &str) -> Result<String> {
    let path = trusted_file_path(root, expected)?;
    std::fs::read_to_string(&path).with_context(|| format!("Failed to read '{}'", path.display()))
}

pub fn write_string_to_root(root: &Path, expected: &str, content: &str) -> Result<()> {
    let path = trusted_file_path(root, expected)?;
    std::fs::create_dir_all(root)
        .with_context(|| format!("Failed to create '{}'", root.display()))?;
    std::fs::write(&path, content).with_context(|| format!("Failed to write '{}'", path.display()))
}
