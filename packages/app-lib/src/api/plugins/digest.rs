//! A summary of a plugin folder's contents, so the user's decision to trust a
//! plugin can be bound to the specific bytes they agreed to.
//!
//! The launcher does not verify signatures; what it can do cheaply is notice
//! that a folder changed under it. A plugin the user enabled is remembered with
//! the digest of its folder at that moment, and every later load compares the
//! two. Files swapped afterwards — by an update, by another program, or by hand
//! — no longer match, and the plugin stops loading until the user looks again.
//!
//! This is a change detector, not a security boundary: it says the bytes are
//! the ones that were agreed to, not that those bytes were ever safe. A plugin
//! still runs with the launcher's own privileges.

use sha2::{Digest, Sha256};
use std::path::Path;

/// Files a file manager creates on its own when someone browses a folder.
///
/// Excluded because including them would break a digest just from opening the
/// plugin directory in Explorer or Finder, which would train the user to
/// re-confirm without reading anything.
const IGNORED_NAMES: [&str; 3] = [".ds_store", "thumbs.db", "desktop.ini"];

/// The digest of a plugin folder, or `None` when it cannot be summarised.
///
/// `None` is a real answer rather than an error: a folder holding something
/// that cannot be read has no digest to compare against, which the caller
/// reports to the user as "this plugin cannot be checked" instead of pretending
/// it matches or pretending it changed.
pub async fn content_digest(dir: &Path) -> crate::Result<Option<String>> {
    let root = dir.to_path_buf();

    // Walking and hashing is blocking IO and CPU work, so keep it off the async
    // runtime: this runs once per plugin on every scan.
    let walked = tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        collect_entries(&root, &root, &mut entries)
            .map(|()| digest_from_entries(&entries))
    })
    .await?;

    match walked {
        Ok(digest) => Ok(Some(digest)),
        Err(error) => {
            tracing::warn!(
                path = %dir.display(),
                "Plugin folder cannot be digested: {error}"
            );
            Ok(None)
        }
    }
}

/// The digest of a list of `(relative path, marker)` entries.
///
/// Split out from the walk so the rule is testable without a filesystem. The
/// entries are sorted by path rather than hashed in traversal order, because
/// the order a directory is read in is not part of the folder's contents —
/// without the sort, copying a tree could change its digest.
fn digest_from_entries(entries: &[(String, String)]) -> String {
    let mut ordered: Vec<&(String, String)> = entries.iter().collect();
    ordered.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();
    for (path, marker) in ordered {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(marker.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn collect_entries(
    root: &Path,
    dir: &Path,
    entries: &mut Vec<(String, String)>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if IGNORED_NAMES.contains(&name.as_str()) {
            continue;
        }

        let path = entry.path();
        let relative = relative_path(root, &path);
        // `file_type` reads the link itself rather than following it, which is
        // what keeps a symlink from pointing the walk somewhere else.
        let file_type = entry.file_type()?;

        if file_type.is_symlink() {
            // The target is recorded rather than followed: swapping a link for
            // another one then shows up as a change, and a link cannot make the
            // digest depend on files outside the plugin.
            let target = std::fs::read_link(&path)?;
            let target = target.to_string_lossy().replace('\\', "/");
            entries.push((relative, format!("l:{target}")));
        } else if file_type.is_dir() {
            collect_entries(root, &path, entries)?;
        } else if file_type.is_file() {
            let bytes = std::fs::read(&path)?;
            entries.push((relative, format!("f:{:x}", Sha256::digest(&bytes))));
        } else {
            // A socket, FIFO or device node: it has no contents to hash, so the
            // entry records that something of that shape sits at this path.
            entries.push((relative, "o:special".to_string()));
        }
    }
    Ok(())
}

/// A path relative to the plugin root, with `/` separators whatever the host
/// uses, so the same tree digests the same on Windows and on Unix.
fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn entry(path: &str, marker: &str) -> (String, String) {
        (path.to_string(), marker.to_string())
    }

    fn write(dir: &Path, relative: &str, contents: &str) {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn digest_of(dir: &Path) -> String {
        let mut entries = Vec::new();
        collect_entries(dir, dir, &mut entries).unwrap();
        digest_from_entries(&entries)
    }

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn the_same_tree_digests_the_same() {
        let dir = temp_dir();
        write(dir.path(), "manifest.json", "{}");
        write(dir.path(), "dist/index.js", "export default 1");

        assert_eq!(digest_of(dir.path()), digest_of(dir.path()));
    }

    #[test]
    fn entry_order_does_not_matter() {
        let forwards = vec![
            entry("a.js", "f:1"),
            entry("b/c.js", "f:2"),
            entry("manifest.json", "f:3"),
        ];
        let backwards = vec![
            entry("manifest.json", "f:3"),
            entry("b/c.js", "f:2"),
            entry("a.js", "f:1"),
        ];

        assert_eq!(digest_from_entries(&forwards), digest_from_entries(&backwards));
    }

    #[test]
    fn a_changed_byte_changes_the_digest() {
        let dir = temp_dir();
        write(dir.path(), "index.js", "export default 1");
        let before = digest_of(dir.path());

        write(dir.path(), "index.js", "export default 2");
        assert_ne!(before, digest_of(dir.path()));
    }

    #[test]
    fn adding_and_removing_files_changes_the_digest() {
        let dir = temp_dir();
        write(dir.path(), "index.js", "a");
        let before = digest_of(dir.path());

        write(dir.path(), "extra.js", "b");
        let added = digest_of(dir.path());
        assert_ne!(before, added);

        fs::remove_file(dir.path().join("extra.js")).unwrap();
        assert_eq!(before, digest_of(dir.path()));
    }

    #[test]
    fn renaming_a_file_changes_the_digest() {
        let dir = temp_dir();
        write(dir.path(), "index.js", "same contents");
        let before = digest_of(dir.path());

        fs::rename(dir.path().join("index.js"), dir.path().join("other.js")).unwrap();
        assert_ne!(before, digest_of(dir.path()));
    }

    #[test]
    fn file_manager_junk_is_ignored() {
        let dir = temp_dir();
        write(dir.path(), "index.js", "a");
        let before = digest_of(dir.path());

        for junk in IGNORED_NAMES {
            write(dir.path(), junk, "junk");
            write(dir.path(), &format!("dist/{junk}"), "junk");
        }

        assert_eq!(before, digest_of(dir.path()));
    }

    #[test]
    fn other_dot_files_are_still_counted() {
        let dir = temp_dir();
        write(dir.path(), "index.js", "a");
        let before = digest_of(dir.path());

        write(dir.path(), ".env", "SECRET=1");
        assert_ne!(
            before,
            digest_of(dir.path()),
            "a dot file is a file like any other and could carry code"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_retargeted_symlink_changes_the_digest() {
        let dir = temp_dir();
        write(dir.path(), "a.js", "a");
        write(dir.path(), "b.js", "b");
        std::os::unix::fs::symlink("a.js", dir.path().join("current.js")).unwrap();
        let before = digest_of(dir.path());

        fs::remove_file(dir.path().join("current.js")).unwrap();
        std::os::unix::fs::symlink("b.js", dir.path().join("current.js")).unwrap();
        assert_ne!(before, digest_of(dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_does_not_pull_in_its_target() {
        let outside = temp_dir();
        write(outside.path(), "secret.txt", "not part of the plugin");

        let dir = temp_dir();
        write(dir.path(), "index.js", "a");
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("link.txt"),
        )
        .unwrap();
        let before = digest_of(dir.path());

        write(outside.path(), "secret.txt", "changed");
        assert_eq!(
            before,
            digest_of(dir.path()),
            "the digest covers the link, not what it points at"
        );
    }

    #[test]
    fn an_unreadable_folder_has_no_digest() {
        let missing = PathBuf::from("this/path/does/not/exist");
        assert!(content_digest_blocking(&missing).is_none());
    }

    /// The blocking half of [`content_digest`], so the failure path is testable
    /// without an async runtime.
    fn content_digest_blocking(dir: &Path) -> Option<String> {
        let mut entries = Vec::new();
        collect_entries(dir, dir, &mut entries)
            .ok()
            .map(|()| digest_from_entries(&entries))
    }

    #[test]
    fn the_digest_names_its_algorithm() {
        let entries = vec![entry("a", "f:1")];
        assert!(digest_from_entries(&entries).starts_with("sha256:"));
    }
}