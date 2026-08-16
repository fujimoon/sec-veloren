//! Pure, Veloren-independent logic for turning a filesystem location into a small
//! "window" of directories to render as dungeon rooms.
//!
//! Given a directory, callers typically want one of three views:
//!
//!   - [`siblings_window`] — up to `2 * radius` *other* directories at the same
//!     level (the current directory's own siblings, i.e. its parent's children),
//!     centered on the current directory's index among them. The current
//!     directory itself is deliberately excluded from this window: callers draw
//!     it as their "current room" separately.
//!   - [`children_window`] — up to `2 * radius + 1` of the current directory's own
//!     children, centered on a caller-chosen "selected" index.
//!   - [`list_children`] — *all* of the current directory's children, unwindowed
//!     (for callers like the ASCII `psy dungeon` browser that render a full,
//!     scrollable list rather than a fixed-size window).
//!
//! Both windows are clamped at the ends of their list rather than wrapping or
//! panicking, and both are built entirely on [`std::fs`] + [`std::path`] — no
//! shell is ever invoked, so directory names containing spaces, parentheses, or
//! non-ASCII characters are handled correctly by construction (unlike the usual
//! `find | xargs basename`-style one-liners this replaces).
//!
//! Hidden directories (name starting with `.`) are always excluded, matching the
//! existing `psy dungeon` ASCII browser's behavior.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

/// One directory as seen from a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirInfo {
    /// Base name (last path component).
    pub name: OsString,
    /// Full, absolute-or-relative-as-given path.
    pub full_path: PathBuf,
    /// 0-based index within the *full* (unwindowed) sorted list this entry came
    /// from — stable regardless of how the window was clamped, so callers can
    /// use it to re-request an adjacent window later.
    pub index: usize,
    /// Number of non-hidden subdirectories this entry itself contains.
    pub subdir_count: usize,
}

fn is_hidden(name: &std::ffi::OsStr) -> bool { name.to_string_lossy().starts_with('.') }

/// Non-hidden subdirectories of `path`, sorted by full path (equivalent to
/// sorting by name, since all entries share the same parent). Entries that can't
/// be read (permission denied, deleted mid-scan, etc.) are silently skipped
/// rather than failing the whole listing — an unreadable directory just behaves
/// as if it were empty. A `path` that doesn't exist or isn't itself readable also
/// yields an empty list rather than an error, for the same reason.
fn list_subdirs(path: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(path) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|p| p.file_name().map(|n| !is_hidden(n)).unwrap_or(true))
        .collect();
    dirs.sort();
    dirs
}

fn to_dir_info(full_path: PathBuf, index: usize) -> DirInfo {
    let name = full_path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| full_path.as_os_str().to_owned());
    let subdir_count = list_subdirs(&full_path).len();
    DirInfo { name, full_path, index, subdir_count }
}

/// *All* non-hidden children of `path`, in sorted order, unwindowed — for
/// callers that render a full scrollable list (the ASCII `psy dungeon`
/// browser) rather than a fixed-size window around a selection.
pub fn list_children(path: &Path) -> Vec<DirInfo> {
    list_subdirs(path)
        .into_iter()
        .enumerate()
        .map(|(i, p)| to_dir_info(p, i))
        .collect()
}

/// Window of `path`'s siblings (its parent's children), `radius` either side of
/// `path`'s own position among them, excluding `path` itself.
///
/// Returns `(total_sibling_count, self_index, window)`. `self_index` is `path`'s
/// 0-based index in the *full* sorted sibling list. If `path` has no parent
/// (e.g. a filesystem root) or isn't found among its parent's non-hidden
/// children (e.g. it's itself hidden), `total_sibling_count` reflects the
/// parent's real sibling count but `self_index` is `0` and the window is empty.
pub fn siblings_window(path: &Path, radius: usize) -> (usize, usize, Vec<DirInfo>) {
    let Some(parent) = path.parent() else {
        return (0, 0, Vec::new());
    };
    let siblings = list_subdirs(parent);
    let total = siblings.len();
    let Some(self_index) = siblings.iter().position(|p| p == path) else {
        return (total, 0, Vec::new());
    };

    let lo = self_index.saturating_sub(radius);
    let hi = (self_index + radius).min(total.saturating_sub(1));
    let window = (lo..=hi)
        .filter(|&i| i != self_index)
        .map(|i| to_dir_info(siblings[i].clone(), i))
        .collect();
    (total, self_index, window)
}

/// Window of `path`'s own children, `radius` either side of `selected` (a 0-based
/// index into the full sorted child list; out-of-range values clamp to the last
/// valid index).
///
/// Returns `(total_child_count, window)`. If `path` has no (non-hidden)
/// subdirectories, both are `0`/empty.
pub fn children_window(path: &Path, selected: usize, radius: usize) -> (usize, Vec<DirInfo>) {
    let children = list_subdirs(path);
    let total = children.len();
    if total == 0 {
        return (0, Vec::new());
    }
    let selected = selected.min(total - 1);
    let lo = selected.saturating_sub(radius);
    let hi = (selected + radius).min(total - 1);
    let window = (lo..=hi).map(|i| to_dir_info(children[i].clone(), i)).collect();
    (total, window)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a temp directory tree: `root/{names...}`, each itself empty, plus
    /// `root/.hidden` to verify hidden-dir exclusion. Returns the root and the
    /// full paths of the (non-hidden) children in the order they were named.
    fn make_siblings(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".hidden")).unwrap();
        let mut paths = Vec::new();
        for name in names {
            let p = root.path().join(name);
            fs::create_dir(&p).unwrap();
            paths.push(p);
        }
        (root, paths)
    }

    #[test]
    fn siblings_window_centers_and_excludes_self() {
        let (_root, paths) = make_siblings(&["a", "b", "c", "d", "e"]);
        // Sorted order is a,b,c,d,e (paths.sort() matches insertion order here).
        let mut sorted = paths.clone();
        sorted.sort();
        let current = &sorted[2]; // "c"

        let (total, self_index, window) = siblings_window(current, 2);
        assert_eq!(total, 5);
        assert_eq!(self_index, 2);
        // radius 2 around index 2 in a 5-element list covers the whole list
        // minus self: indices 0,1,3,4.
        let got: Vec<usize> = window.iter().map(|d| d.index).collect();
        assert_eq!(got, vec![0, 1, 3, 4]);
        assert!(window.iter().all(|d| d.full_path != *current));
    }

    #[test]
    fn siblings_window_clamps_at_start() {
        let (_root, paths) = make_siblings(&["a", "b", "c", "d", "e"]);
        let mut sorted = paths.clone();
        sorted.sort();
        let current = &sorted[0]; // "a", index 0

        let (total, self_index, window) = siblings_window(current, 2);
        assert_eq!(total, 5);
        assert_eq!(self_index, 0);
        let got: Vec<usize> = window.iter().map(|d| d.index).collect();
        // window would be -2..=2 clamped to 0..=2, minus self (0) -> 1,2
        assert_eq!(got, vec![1, 2]);
    }

    #[test]
    fn siblings_window_clamps_at_end() {
        let (_root, paths) = make_siblings(&["a", "b", "c", "d", "e"]);
        let mut sorted = paths.clone();
        sorted.sort();
        let current = &sorted[4]; // "e", index 4

        let (total, self_index, window) = siblings_window(current, 2);
        assert_eq!(total, 5);
        assert_eq!(self_index, 4);
        let got: Vec<usize> = window.iter().map(|d| d.index).collect();
        assert_eq!(got, vec![2, 3]);
    }

    #[test]
    fn handles_spaces_parens_and_unicode_names() {
        let names = ["a dir with spaces", "paren (dir)", "日本語ディレクトリ", "z"];
        let (_root, paths) = make_siblings(&names);
        let mut sorted = paths.clone();
        sorted.sort();

        // Pick one entry as "current" and confirm every *other* name round-trips
        // through the window untouched (no shell tokenization to mangle spaces
        // or parens, no encoding loss for non-ASCII names).
        let current = &sorted[1];
        let (_total, _self_index, window) = siblings_window(current, 3);
        let got_names: Vec<String> =
            window.iter().map(|d| d.name.to_string_lossy().into_owned()).collect();

        let current_name = current.file_name().unwrap().to_string_lossy().into_owned();
        let expected: Vec<&str> = names.iter().copied().filter(|&n| n != current_name).collect();
        assert_eq!(expected.len(), names.len() - 1);
        for name in expected {
            assert!(got_names.iter().any(|g| g == name), "missing {name} in {got_names:?}");
        }
    }

    #[test]
    fn hidden_directories_are_excluded() {
        let (_root, paths) = make_siblings(&["a", "b"]);
        let mut sorted = paths.clone();
        sorted.sort();
        let (total, _self_index, _window) = siblings_window(&sorted[0], 5);
        // Only "a" and "b" count; ".hidden" must not be included.
        assert_eq!(total, 2);
    }

    #[test]
    fn nonexistent_path_yields_empty_children_window() {
        let bogus = PathBuf::from("/this/does/not/exist/hopefully-ever-xyz");
        let (total, window) = children_window(&bogus, 0, 2);
        assert_eq!(total, 0);
        assert!(window.is_empty());
    }

    #[test]
    fn children_window_reports_subdir_counts() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::create_dir(child.join("grandchild1")).unwrap();
        fs::create_dir(child.join("grandchild2")).unwrap();

        let (total, window) = children_window(root.path(), 0, 2);
        assert_eq!(total, 1);
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].subdir_count, 2);
    }

    #[test]
    fn list_children_returns_all_unwindowed() {
        let (_root, paths) = make_siblings(&["a", "b", "c", "d", "e"]);
        let parent = paths[0].parent().unwrap();
        let all = list_children(parent);
        assert_eq!(all.len(), 5);
        assert_eq!(all.iter().map(|d| d.index).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn children_window_clamps_selected_index() {
        let (_root, paths) = make_siblings(&["a", "b", "c"]);
        let parent = paths[0].parent().unwrap();
        // selected far out of range should clamp to the last valid index, not panic.
        let (total, window) = children_window(parent, 999, 1);
        assert_eq!(total, 3);
        assert!(!window.is_empty());
    }
}
