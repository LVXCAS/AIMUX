use codex_utils_absolute_path::AbsolutePathBuf;
use dirs::home_dir;
use std::path::PathBuf;

/// Returns the path to the AIMUX configuration directory.
///
/// Resolution order:
///   1. `AIMUX_HOME` environment variable (new name, preferred)
///   2. `CODEX_HOME` environment variable (legacy fallback for backwards compat)
///   3. `~/.aimux` — default new location
///   4. `~/.codex` — legacy fallback if it already exists on disk (migration aid)
///
/// When an env var is set, the value must point to an existing directory.
/// The path will be canonicalized; this function returns Err otherwise.
pub fn find_codex_home() -> std::io::Result<AbsolutePathBuf> {
    // AIMUX_HOME takes precedence; CODEX_HOME is the legacy alias.
    let aimux_home_env = std::env::var("AIMUX_HOME")
        .ok()
        .filter(|val| !val.is_empty());
    let codex_home_env = std::env::var("CODEX_HOME")
        .ok()
        .filter(|val| !val.is_empty());
    let env_override = aimux_home_env.or(codex_home_env);
    find_codex_home_from_env(env_override.as_deref())
}

fn resolve_env_home(val: &str, var_name: &str) -> std::io::Result<AbsolutePathBuf> {
    let path = PathBuf::from(val);
    let metadata = std::fs::metadata(&path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{var_name} points to {val:?}, but that path does not exist"),
        ),
        _ => std::io::Error::new(
            err.kind(),
            format!("failed to read {var_name} {val:?}: {err}"),
        ),
    })?;

    if !metadata.is_dir() {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{var_name} points to {val:?}, but that path is not a directory"),
        ))
    } else {
        let canonical = path.canonicalize().map_err(|err| {
            std::io::Error::new(
                err.kind(),
                format!("failed to canonicalize {var_name} {val:?}: {err}"),
            )
        })?;
        AbsolutePathBuf::from_absolute_path(canonical)
    }
}

fn find_codex_home_from_env(home_env: Option<&str>) -> std::io::Result<AbsolutePathBuf> {
    // Honor env override when set, using a generic error label.
    if let Some(val) = home_env {
        return resolve_env_home(val, "AIMUX_HOME/CODEX_HOME");
    }

    let base = home_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not find home directory",
        )
    })?;

    // Prefer ~/.aimux; fall back to ~/.codex if it already exists (migration aid).
    let aimux_dir = base.join(".aimux");
    let codex_dir = base.join(".codex");
    if !aimux_dir.exists() && codex_dir.exists() {
        AbsolutePathBuf::from_absolute_path(codex_dir)
    } else {
        AbsolutePathBuf::from_absolute_path(aimux_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::find_codex_home_from_env;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use dirs::home_dir;
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::io::ErrorKind;
    use tempfile::TempDir;

    #[test]
    fn find_codex_home_env_missing_path_is_fatal() {
        let temp_home = TempDir::new().expect("temp home");
        let missing = temp_home.path().join("missing-aimux-home");
        let missing_str = missing
            .to_str()
            .expect("missing aimux home path should be valid utf-8");

        let err = find_codex_home_from_env(Some(missing_str)).expect_err("missing AIMUX_HOME");
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(
            err.to_string().contains("AIMUX_HOME"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn find_codex_home_env_file_path_is_fatal() {
        let temp_home = TempDir::new().expect("temp home");
        let file_path = temp_home.path().join("aimux-home.txt");
        fs::write(&file_path, "not a directory").expect("write temp file");
        let file_str = file_path
            .to_str()
            .expect("file aimux home path should be valid utf-8");

        let err = find_codex_home_from_env(Some(file_str)).expect_err("file AIMUX_HOME");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(
            err.to_string().contains("not a directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn find_codex_home_env_valid_directory_canonicalizes() {
        let temp_home = TempDir::new().expect("temp home");
        let temp_str = temp_home
            .path()
            .to_str()
            .expect("temp aimux home path should be valid utf-8");

        let resolved = find_codex_home_from_env(Some(temp_str)).expect("valid AIMUX_HOME");
        let expected = temp_home
            .path()
            .canonicalize()
            .expect("canonicalize temp home");
        let expected = AbsolutePathBuf::from_absolute_path(expected).expect("absolute home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn find_codex_home_without_env_uses_aimux_dir_by_default() {
        // When neither ~/.aimux nor ~/.codex exists, we expect ~/.aimux.
        let resolved =
            find_codex_home_from_env(/*home_env*/ None).expect("default AIMUX_HOME");
        let mut expected = home_dir().expect("home dir");
        // The actual result depends on whether ~/.codex exists on this machine.
        // We just assert the result is either ~/.aimux or ~/.codex.
        let aimux = {
            let mut p = expected.clone();
            p.push(".aimux");
            p
        };
        expected.push(".codex");
        let resolved_path = resolved.as_path();
        assert!(
            resolved_path == aimux.as_path() || resolved_path == expected.as_path(),
            "expected ~/.aimux or ~/.codex, got {resolved_path:?}"
        );
    }
}
