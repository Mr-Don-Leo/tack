//! Attachments.
//!
//! Files are copied into an app-managed store rather than referenced in place,
//! so a task keeps working after the original is moved. Names coming from the
//! filesystem are treated as untrusted: the stored filename is rebuilt from a
//! fresh UUID plus a sanitized basename, which makes `../` traversal and
//! absolute paths impossible by construction.

use std::fs;
use std::path::{Component, Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{Result, not_found, rejected};
use crate::models::Attachment;
use crate::util::{new_id, now};

/// Refuse anything larger; attachments live inside the user's app data.
const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

pub fn map(row: &Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        name: row.get("name")?,
        path: row.get("path")?,
        size: row.get("size")?,
        mime: row.get("mime")?,
        created_at: row.get("created_at")?,
    })
}

const SELECT: &str = "SELECT id, task_id, name, path, size, mime, created_at FROM attachments";

pub fn for_task(conn: &Connection, task_id: &str) -> Result<Vec<Attachment>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE task_id = ?1 ORDER BY created_at ASC"))?;
    Ok(stmt
        .query_map(params![task_id], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Attachment> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), params![id], map)
        .optional()?
        .ok_or_else(|| not_found("attachment"))
}

/// Copies `source` into the managed store and records it against `task_id`.
pub fn attach_file(
    conn: &Connection,
    data_dir: &Path,
    task_id: &str,
    source: &Path,
) -> Result<Attachment> {
    let meta = fs::metadata(source)?;
    if !meta.is_file() {
        return Err(rejected("Only files can be attached"));
    }
    if meta.len() > MAX_ATTACHMENT_BYTES {
        return Err(rejected("Attachments are limited to 50 MB"));
    }
    // Confirms the task exists before anything lands on disk.
    super::tasks::get(conn, task_id)?;

    let display_name = safe_display_name(source);
    let stored_name = format!("{}-{}", new_id(), safe_file_stem(&display_name));
    let dir = task_dir(data_dir, task_id)?;
    fs::create_dir_all(&dir)?;
    let dest = dir.join(&stored_name);

    // Belt and braces: the assembled path must still be inside the store.
    let root = crate::db::attachments_dir(data_dir);
    if !dest.starts_with(&root) {
        return Err(rejected("Refusing to write outside the attachment store"));
    }
    fs::copy(source, &dest)?;

    let id = new_id();
    conn.execute(
        "INSERT INTO attachments (id, task_id, name, path, size, mime, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            task_id,
            display_name,
            dest.to_string_lossy(),
            meta.len() as i64,
            guess_mime(&display_name),
            now()
        ],
    )?;
    get(conn, &id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let attachment = get(conn, id)?;
    let _ = fs::remove_file(&attachment.path);
    conn.execute("DELETE FROM attachments WHERE id = ?1", params![id])?;
    Ok(())
}

/// Removes every file belonging to a task, then its rows.
pub fn delete_all_for_task(conn: &Connection, task_id: &str) -> Result<()> {
    for attachment in for_task(conn, task_id)? {
        let _ = fs::remove_file(&attachment.path);
    }
    conn.execute("DELETE FROM attachments WHERE task_id = ?1", params![task_id])?;
    Ok(())
}

/// Deletes attachment files with no surviving row. Runs at startup so a crash
/// between `fs::copy` and the insert cannot leak storage forever.
pub fn prune_orphans(conn: &Connection, data_dir: &Path) -> Result<usize> {
    let root = crate::db::attachments_dir(data_dir);
    if !root.exists() {
        return Ok(0);
    }
    let mut stmt = conn.prepare("SELECT path FROM attachments")?;
    let known: std::collections::HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;

    let mut removed = 0;
    for task_dir in fs::read_dir(&root)?.filter_map(|e| e.ok()) {
        if !task_dir.path().is_dir() {
            continue;
        }
        for file in fs::read_dir(task_dir.path())?.filter_map(|e| e.ok()) {
            if !known.contains(&file.path().to_string_lossy().to_string())
                && fs::remove_file(file.path()).is_ok()
            {
                removed += 1;
            }
        }
        // Tidy up the directory once its last file is gone.
        let _ = fs::remove_dir(task_dir.path());
    }
    Ok(removed)
}

/// `<store>/<task id>` — the task id is a UUID we generated, but it is
/// re-validated here so a tampered database cannot escape the store.
fn task_dir(data_dir: &Path, task_id: &str) -> Result<PathBuf> {
    if task_id.is_empty()
        || !task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(rejected("Invalid task identifier"));
    }
    Ok(crate::db::attachments_dir(data_dir).join(task_id))
}

/// The basename of `source`, with any directory structure discarded.
fn safe_display_name(source: &Path) -> String {
    let name = source
        .components()
        .next_back()
        .and_then(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .unwrap_or_default();
    let trimmed = name.trim();
    if trimmed.is_empty() { "attachment".to_string() } else { trimmed.to_string() }
}

/// Reduces a name to characters that are safe on every target filesystem.
fn safe_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Leading dots would hide the file; runs of dots could still read as `..`.
    let cleaned = cleaned.trim_matches(['.', ' ']).to_string();
    let cleaned = cleaned.replace("..", "_");
    let capped: String = cleaned.chars().take(100).collect();
    if capped.is_empty() { "file".to_string() } else { capped }
}

fn guess_mime(name: &str) -> Option<String> {
    let ext = Path::new(name).extension()?.to_string_lossy().to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => return None,
    };
    Some(mime.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_drops_directories() {
        assert_eq!(safe_display_name(Path::new("/etc/passwd")), "passwd");
        assert_eq!(safe_display_name(Path::new("../../secret.txt")), "secret.txt");
    }

    #[test]
    fn file_stem_neutralises_traversal_and_separators() {
        let stem = safe_file_stem("../../etc/passwd");
        assert!(!stem.contains(".."), "{stem} still contains a traversal segment");
        assert!(!stem.contains('/'), "{stem} still contains a separator");
        assert!(!safe_file_stem("a\\b").contains('\\'));
        assert!(!safe_file_stem("C:\\Windows\\system32").contains(':'));
    }

    #[test]
    fn file_stem_never_returns_empty() {
        assert_eq!(safe_file_stem("..."), "file");
        assert_eq!(safe_file_stem("   "), "file");
    }

    #[test]
    fn task_dir_rejects_traversal() {
        let root = Path::new("/tmp/tack-test");
        assert!(task_dir(root, "../escape").is_err());
        assert!(task_dir(root, "9f8e-1234").is_ok());
    }
}
