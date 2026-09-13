//! Fd-relative helpers + the single owned deleter used by daemon-down `rm`
//! and `clean-artifacts`. Never a weaker sibling of `grove_git::delete_owned`.
pub fn is_safe_worktree_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains('\0')
}
#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    /// Records a `wt_create_state` journal row (same schema as
    /// `liveness::tests::write_create_state`) so daemon liveness sees the id.
    /// Tests that exercise remove/GC plant a journal the same way the daemon
    /// would after a successful create.
    pub(crate) fn plant_journal(
        data: &Path,
        worktree_id: &str,
        backing: &Path,
        phase: Option<&str>,
    ) {
        std::fs::create_dir_all(data).unwrap();
        let conn = rusqlite::Connection::open(data.join("daemon.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS wt_create_state (
                worktree_id TEXT PRIMARY KEY,
                phase TEXT NOT NULL,
                dest TEXT NOT NULL,
                source TEXT NOT NULL,
                orphan_seen_at INTEGER,
                updated_at INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO wt_create_state(worktree_id, phase, dest, source, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![
                worktree_id,
                phase.unwrap_or("pinned"),
                backing.display().to_string(),
                backing.display().to_string(),
            ],
        )
        .unwrap();
    }
}
