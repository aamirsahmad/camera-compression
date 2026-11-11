//! Turning a source argument into a list of input files.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Resolve the source argument into a sorted list of inputs.
///
/// A directory is walked recursively for jpgs; a file is read as a
/// newline-delimited list of paths.
///
/// # Errors
///
/// Returns [`Error::InvalidSource`] if `src` is neither, or [`Error::Io`] if a
/// directory cannot be listed or a list file cannot be read.
pub fn collect(src: &Path) -> Result<Vec<PathBuf>> {
    if src.is_dir() {
        walk(src)
    } else if src.is_file() {
        read_list(src)
    } else {
        Err(Error::InvalidSource(src.to_path_buf()))
    }
}

/// Equivalent of `find <root> -type f -iname '*.jpg'`.
///
/// Symlinks are not followed, matching `find`'s default. Results are sorted so
/// that progress and resume behave identically across runs.
///
/// # Errors
///
/// Returns [`Error::Io`] if any directory in the tree cannot be listed.
pub fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    // Iterative rather than recursive, so a deep tree cannot blow the stack.
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(&dir, e))?;
            let path = entry.path();
            // `file_type` does not traverse symlinks, so a symlinked directory
            // is neither descended into nor mistaken for a regular file.
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() && is_jpg(&path) => found.push(path),
                _ => {}
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Case-insensitive `.jpg`, matching `find -iname '*.jpg'`.
///
/// This deliberately does not match `.jpeg`, exactly as that `find` would not.
#[must_use]
pub fn is_jpg(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg"))
}

/// Read a newline-delimited list of paths, skipping blanks and `#` comments.
///
/// # Errors
///
/// Returns [`Error::Io`] if the file cannot be read.
pub fn read_list(path: &Path) -> Result<Vec<PathBuf>> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(PathBuf::from)
        .collect())
}

/// Find inputs that would produce the same output name.
///
/// Output names are flat file stems, so two inputs in different
/// subdirectories can collide. Returned stems are sorted and deduplicated.
#[must_use]
pub fn colliding_stems(files: &[PathBuf]) -> Vec<String> {
    let mut stems: Vec<String> = files
        .iter()
        .filter_map(|f| f.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    stems.sort();

    let mut dupes: Vec<String> = stems
        .windows(2)
        .filter(|w| w[0] == w[1])
        .map(|w| w[0].clone())
        .collect();
    dupes.dedup();
    dupes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// True for Sony raw sidecars, which sit beside the jpgs and must be skipped.
    fn is_raw(name: &str) -> bool {
        std::path::Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("arw"))
    }

    /// Create `relative` under `root`, parents included, with dummy contents.
    fn touch(root: &Path, relative: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("relative path needs a parent")).unwrap();
        fs::write(&path, b"x").unwrap();
        path
    }

    #[test]
    fn is_jpg_ignores_extension_case() {
        for name in ["a.jpg", "a.JPG", "a.Jpg", "a.jPg"] {
            assert!(is_jpg(Path::new(name)), "{name} should match");
        }
    }

    #[test]
    fn is_jpg_rejects_other_extensions_including_jpeg() {
        // `.jpeg` is excluded on purpose: `find -iname '*.jpg'` would skip it.
        for name in ["a.jpeg", "a.JPEG", "a.png", "a.arw", "a", "a.jpg.bak"] {
            assert!(!is_jpg(Path::new(name)), "{name} should not match");
        }
    }

    #[test]
    fn walk_finds_jpgs_at_any_depth_and_ignores_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "top.jpg");
        touch(root, "a/one.JPG");
        touch(root, "a/b/c/d/deep.jpg");
        touch(root, "a/raw.ARW");
        touch(root, "a/b/note.txt");

        let found = walk(root).unwrap();
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert_eq!(found.len(), 3, "got {names:?}");
        assert!(names.contains(&"deep.jpg".to_string()));
        assert!(names.contains(&"one.JPG".to_string()));
        assert!(
            !names.iter().any(|n| is_raw(n)),
            "raw files must be skipped"
        );
    }

    #[test]
    fn walk_returns_sorted_paths_for_deterministic_resume() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["c.jpg", "a.jpg", "b.jpg", "z/y.jpg"] {
            touch(root, name);
        }

        let found = walk(root).unwrap();
        let mut sorted = found.clone();
        sorted.sort();
        assert_eq!(found, sorted);
    }

    #[test]
    fn walk_on_empty_tree_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(walk(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn walk_reports_the_offending_path_when_unreadable() {
        let missing = Path::new("/definitely/not/here");
        match walk(missing) {
            Err(Error::Io { path, .. }) => assert_eq!(path, missing),
            other => panic!("expected Io error, got {other:?}"),
        }
    }

    #[test]
    fn read_list_skips_blanks_comments_and_surrounding_space() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("list.txt");
        fs::write(
            &list,
            "/a/one.jpg\n\n   \n# a comment\n  /b/two.jpg  \n\t# indented comment\n",
        )
        .unwrap();

        let got = read_list(&list).unwrap();
        assert_eq!(
            got,
            vec![PathBuf::from("/a/one.jpg"), PathBuf::from("/b/two.jpg")]
        );
    }

    #[test]
    fn read_list_on_empty_file_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("empty.txt");
        fs::write(&list, "").unwrap();
        assert!(read_list(&list).unwrap().is_empty());
    }

    #[test]
    fn collect_dispatches_on_whether_source_is_a_dir_or_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "shot.jpg");

        // directory -> walked
        assert_eq!(collect(root).unwrap().len(), 1);

        // file -> read as a list
        let list = root.join("list.txt");
        fs::write(&list, format!("{}\n", root.join("shot.jpg").display())).unwrap();
        assert_eq!(collect(&list).unwrap().len(), 1);
    }

    #[test]
    fn collect_rejects_a_path_that_is_neither() {
        let missing = Path::new("/no/such/source");
        match collect(missing) {
            Err(Error::InvalidSource(p)) => assert_eq!(p, missing),
            other => panic!("expected InvalidSource, got {other:?}"),
        }
    }

    #[test]
    fn colliding_stems_finds_repeats_across_directories() {
        let files: Vec<PathBuf> = ["a/S.jpg", "b/S.jpg", "c/T.jpg", "d/T.JPG", "e/U.jpg"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(colliding_stems(&files), vec!["S", "T"]);
    }

    #[test]
    fn colliding_stems_reports_each_repeat_once() {
        let files: Vec<PathBuf> = ["a/S.jpg", "b/S.jpg", "c/S.jpg"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(colliding_stems(&files), vec!["S"]);
    }

    #[test]
    fn colliding_stems_is_empty_when_names_are_unique() {
        let files: Vec<PathBuf> = ["a/one.jpg", "b/two.jpg"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert!(colliding_stems(&files).is_empty());
    }
}
