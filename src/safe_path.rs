use std::path::{Component, Path, PathBuf};

use crate::errors::ServerError;

/// Join `user_path` onto `base`, rejecting any user input that could escape `base`.
/// Only `Normal` components are allowed — `..`, `.`, absolute roots, and Windows
/// path prefixes are refused. We don't use `canonicalize` because the target may
/// not exist yet (e.g. when creating a new upload).
pub fn safe_join(base: &Path, user_path: &str) -> Result<PathBuf, ServerError> {
    let user_path = Path::new(user_path);
    for component in user_path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(ServerError::InvalidPath);
        }
    }
    Ok(base.join(user_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from("/store")
    }

    fn assert_accepts(input: &str, expected: &str) {
        match safe_join(&base(), input) {
            Ok(p) => assert_eq!(p, PathBuf::from(expected), "input={input:?}"),
            Err(_) => panic!("expected accept for {input:?}"),
        }
    }

    fn assert_rejects(input: &str) {
        match safe_join(&base(), input) {
            Err(ServerError::InvalidPath) => {}
            Err(_) => panic!("expected InvalidPath for {input:?}, got a different error"),
            Ok(p) => panic!("expected reject for {input:?}, got {p:?}"),
        }
    }

    #[test]
    fn allows_empty_path() {
        assert_accepts("", "/store");
    }

    #[test]
    fn allows_single_component() {
        assert_accepts("file.txt", "/store/file.txt");
    }

    #[test]
    fn allows_nested_components() {
        assert_accepts("a/b/c.txt", "/store/a/b/c.txt");
    }

    #[test]
    fn allows_trailing_slash() {
        assert_accepts("subdir/", "/store/subdir");
    }

    #[test]
    fn allows_hidden_file() {
        assert_accepts(".hidden", "/store/.hidden");
    }

    #[test]
    fn allows_dots_inside_name() {
        // `..bar` is a Normal component, not a ParentDir.
        assert_accepts("foo/..bar", "/store/foo/..bar");
        assert_accepts("file..txt", "/store/file..txt");
    }

    #[test]
    fn rejects_parent_dir_alone() {
        assert_rejects("..");
    }

    #[test]
    fn rejects_parent_dir_leading() {
        assert_rejects("../etc/hostname");
        assert_rejects("../../etc/hostname");
    }

    #[test]
    fn rejects_parent_dir_in_middle() {
        assert_rejects("a/../b");
        assert_rejects("a/b/../../c");
    }

    #[test]
    fn rejects_parent_dir_trailing() {
        assert_rejects("a/..");
    }

    #[test]
    fn rejects_current_dir() {
        // `.` only produces a CurDir component when it's at the very start of the
        // path; interior `.` is normalized away by `Path::components`.
        assert_rejects(".");
        assert_rejects("./foo");
    }

    #[test]
    fn allows_interior_current_dir_normalized_away() {
        assert_accepts("foo/./bar", "/store/foo/bar");
    }

    #[test]
    fn rejects_absolute_path() {
        assert_rejects("/etc/hostname");
        assert_rejects("/");
    }

    #[test]
    fn rejects_double_leading_slash() {
        assert_rejects("//etc/hostname");
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_drive_prefix() {
        assert_rejects(r"C:\windows\system32");
        assert_rejects(r"C:foo");
    }
}
