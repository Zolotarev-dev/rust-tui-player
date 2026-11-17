//! Track discovery with recursive scanning, extension filtering, and symlink safety.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const ALLOWED_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "wav"];

/// A discovered audio track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub path: PathBuf,
    /// File name as a plain string (lossy) for display/sorting.
    pub name: String,
}

/// Discover supported audio tracks under `root`.
///
/// Safety properties:
/// - Recurses into directories without following symlink loops.
/// - Does not follow symlinks whose resolved target is outside the canonicalized
///   target directory.
/// - Collects only files with extensions: mp3, flac, ogg, wav (case-insensitive).
pub fn discover_tracks(root: &Path) -> Result<Vec<Track>, String> {
    let canonical_root = fs::canonicalize(root)
        .map_err(|e| format!("Failed to canonicalize root '{}': {e}", root.display()))?;
    if !canonical_root.is_dir() {
        return Err(format!(
            "Target path '{}' is not a directory",
            canonical_root.display()
        ));
    }

    let mut tracks = Vec::new();
    let mut visited_dirs: HashSet<PathBuf> = HashSet::new();
    walk_dir(&canonical_root, &canonical_root, &mut visited_dirs, &mut tracks)?;

    tracks.sort_by_key(|a| a.name.to_lowercase());
    Ok(tracks)
}

fn walk_dir(
    dir: &Path,
    canonical_root: &Path,
    visited_dirs: &mut HashSet<PathBuf>,
    tracks: &mut Vec<Track>,
) -> Result<(), String> {
    let canonical_dir = fs::canonicalize(dir)
        .map_err(|e| format!("Failed to canonicalize directory '{}': {e}", dir.display()))?;

    // Do not traverse outside the target root.
    if !canonical_dir.starts_with(canonical_root) {
        return Ok(());
    }

    // Prevent symlink loops / repeated traversal.
    if !visited_dirs.insert(canonical_dir.clone()) {
        return Ok(());
    }

    let entries = fs::read_dir(&canonical_dir)
        .map_err(|e| format!("Failed to read directory '{}': {e}", canonical_dir.display()))?;

    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let file_type = meta.file_type();

        if file_type.is_symlink() {
            // Resolve symlink and ensure its target stays within root.
            let resolved = match fs::canonicalize(&path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !resolved.starts_with(canonical_root) {
                continue;
            }
            let resolved_meta = match fs::metadata(&resolved) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if resolved_meta.is_dir() {
                walk_dir(&resolved, canonical_root, visited_dirs, tracks)?;
            } else if resolved_meta.is_file() && is_supported_audio(&resolved) {
                tracks.push(track_from_path(&resolved));
            }
            continue;
        }

        if file_type.is_dir() {
            // Extra guard: canonicalize and ensure inside root.
            if let Ok(canon_sub) = fs::canonicalize(&path) {
                if canon_sub.starts_with(canonical_root) {
                    walk_dir(&canon_sub, canonical_root, visited_dirs, tracks)?;
                }
            }
            continue;
        }

        if file_type.is_file() && is_supported_audio(&path) {
            // Use canonicalized path for consistency.
            let canon_file = match fs::canonicalize(&path) {
                Ok(p) => p,
                Err(_) => path,
            };
            if canon_file.starts_with(canonical_root) {
                tracks.push(track_from_path(&canon_file));
            }
        }
    }

    Ok(())
}

fn is_supported_audio(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            ALLOWED_EXTENSIONS.iter().any(|&allowed| allowed == lower)
        })
        .unwrap_or(false)
}

fn track_from_path(path: &Path) -> Track {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    Track {
        path: path.to_path_buf(),
        name,
    }
}

/// Exposed for testing: check if extension is considered supported.
pub fn is_supported_extension(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    ALLOWED_EXTENSIONS.iter().any(|&allowed| allowed == lower)
}

#[cfg(test)]
mod tests {
    use super::is_supported_extension;

    #[test]
    fn supported_extensions_case_insensitive() {
        assert!(is_supported_extension("mp3"));
        assert!(is_supported_extension("MP3"));
        assert!(is_supported_extension("flac"));
        assert!(is_supported_extension("OGG"));
        assert!(is_supported_extension("wav"));
    }

    #[test]
    fn unsupported_extensions() {
        assert!(!is_supported_extension("txt"));
        assert!(!is_supported_extension("m4a"));
        assert!(!is_supported_extension("aac"));
        assert!(!is_supported_extension(""));
    }
}
