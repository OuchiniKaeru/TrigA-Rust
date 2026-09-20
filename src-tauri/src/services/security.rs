use crate::AppError;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct SecurityPolicy {
    pub allowed_paths: Vec<PathBuf>,
    pub allowed_executables: Vec<String>,
}

impl SecurityPolicy {
    pub fn validate_path(&self, path: &Path) -> Result<PathBuf, AppError> {
        // Resolve existing paths via canonicalize; for not-yet-existing
        // destinations (copy/move targets) canonicalize the nearest
        // existing ancestor and join the remainder lexically.
        let normalized = canonicalize_lossy(path);
        if !self.allowed_paths.is_empty()
            && !self
                .allowed_paths
                .iter()
                .any(|root| normalized.starts_with(root))
        {
            return Err(AppError::Security(format!(
                "path is outside allowed roots: {}",
                normalized.display()
            )));
        }
        Ok(normalized)
    }

    pub fn validate_executable(&self, executable: &str) -> Result<(), AppError> {
        if executable.trim().is_empty() {
            return Err(AppError::Security("executable must not be empty".into()));
        }
        // Reject shell metacharacters: executables are invoked without a shell.
        if executable.contains([';', '&', '|', '`', '$', '(', ')', '<', '>', '\n']) {
            return Err(AppError::Security(format!(
                "executable contains forbidden characters: {executable}"
            )));
        }
        if self.allowed_executables.is_empty()
            || self
                .allowed_executables
                .iter()
                .any(|value| value == executable)
        {
            Ok(())
        } else {
            Err(AppError::Security(format!(
                "executable is not allowed: {executable}"
            )))
        }
    }
}

fn canonicalize_lossy(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    // Walk up to the nearest existing ancestor.
    let mut current = path;
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canonical) = current.canonicalize() {
            let mut result = canonical;
            for component in suffix.iter().rev() {
                result.push(component);
            }
            return result;
        }
        match (current.file_name(), current.parent()) {
            (Some(name), Some(parent)) => {
                suffix.push(name.to_os_string());
                current = parent;
            }
            _ => {
                // No existing ancestor; fall back to absolute lexical path.
                if path.is_absolute() {
                    return path.to_path_buf();
                }
                if let Ok(cwd) = std::env::current_dir() {
                    return cwd.join(path);
                }
                return path.to_path_buf();
            }
        }
    }
}

/// Lexical fallback used only when validation must not fail on
/// not-yet-existing paths during definition checks.
pub fn unchecked_path(input: &str) -> PathBuf {
    let path = Path::new(input);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_disallowed_executable() {
        let policy = SecurityPolicy {
            allowed_executables: vec!["python3".into()],
            ..Default::default()
        };
        assert!(policy.validate_executable("rm").is_err());
    }
}
