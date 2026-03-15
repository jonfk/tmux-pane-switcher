use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, params};
use tps_core::{JumpTarget, PaneSnapshot, ProcessClass, RankedPane, classify_command};

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create database directory {}", parent.display())
            })?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("failed to open database at {}", path.display()))?;
        let mut store = Self { conn };
        store.init()?;
        Ok(store)
    }

    pub fn upsert_snapshots(&mut self, snapshots: &[PaneSnapshot]) -> Result<usize> {
        if snapshots.is_empty() {
            return Ok(0);
        }

        let server_key = &snapshots[0].server_key;
        let socket_path = &snapshots[0].socket_path;
        let server_start_time = snapshots[0].server_start_time;
        if snapshots
            .iter()
            .any(|snapshot| snapshot.server_key != *server_key)
        {
            bail!("cannot persist snapshots from multiple tmux servers in one batch");
        }

        let tx = self.conn.transaction()?;
        let observed_at = snapshots[0].observed_at.clone();

        tx.execute(
            "INSERT INTO servers(server_key, socket_path, start_time, created_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(server_key) DO UPDATE SET
                socket_path = excluded.socket_path,
                start_time = excluded.start_time,
                last_seen_at = excluded.last_seen_at",
            params![server_key, socket_path, server_start_time, observed_at],
        )?;
        prune_stale_servers(&tx, server_key, socket_path)?;
        let server_id: i64 = tx.query_row(
            "SELECT id FROM servers WHERE server_key = ?1",
            params![server_key],
            |row| row.get(0),
        )?;

        let mut seen_panes = Vec::with_capacity(snapshots.len());
        for snapshot in snapshots {
            seen_panes.push(snapshot.pane_id.clone());

            tx.execute(
                "INSERT INTO panes(
                    server_id,
                    tmux_pane_id,
                    tmux_window_id,
                    tmux_session_id,
                    session_name,
                    window_name,
                    window_index,
                    pane_index,
                    first_seen_at,
                    last_seen_at,
                    last_known_title,
                    last_known_cwd,
                    last_known_command,
                    is_alive
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(server_id, tmux_pane_id) DO UPDATE SET
                    tmux_window_id = excluded.tmux_window_id,
                    tmux_session_id = excluded.tmux_session_id,
                    session_name = excluded.session_name,
                    window_name = excluded.window_name,
                    window_index = excluded.window_index,
                    pane_index = excluded.pane_index,
                    last_seen_at = excluded.last_seen_at,
                    last_known_title = excluded.last_known_title,
                    last_known_cwd = excluded.last_known_cwd,
                    last_known_command = excluded.last_known_command,
                    is_alive = excluded.is_alive",
                params![
                    server_id,
                    snapshot.pane_id,
                    snapshot.window_id,
                    snapshot.session_id,
                    snapshot.session_name,
                    snapshot.window_name,
                    snapshot.window_index,
                    snapshot.pane_index,
                    snapshot.observed_at,
                    snapshot.pane_title,
                    snapshot.pane_current_path,
                    snapshot.pane_current_command,
                    bool_to_sqlite(!snapshot.pane_dead),
                ],
            )?;

            let pane_id: i64 = tx.query_row(
                "SELECT id FROM panes WHERE server_id = ?1 AND tmux_pane_id = ?2",
                params![server_id, snapshot.pane_id],
                |row| row.get(0),
            )?;

            let process_class = classify_command(&snapshot.pane_current_command);
            let raw_payload = serde_json::to_string(snapshot)?;

            tx.execute(
                "INSERT INTO events(
                    server_id,
                    pane_id,
                    event_type,
                    event_ts,
                    pane_pid,
                    observed_command,
                    process_class,
                    raw_payload
                 ) VALUES (?1, ?2, 'pane_snapshot', ?3, ?4, ?5, ?6, ?7)",
                params![
                    server_id,
                    pane_id,
                    snapshot.observed_at,
                    snapshot.pane_pid,
                    snapshot.pane_current_command,
                    process_class.as_str(),
                    raw_payload,
                ],
            )?;

            tx.execute(
                "INSERT INTO pane_state(
                    pane_id,
                    server_id,
                    tmux_session_id,
                    tmux_window_id,
                    tmux_pane_id,
                    session_name,
                    window_index,
                    window_name,
                    pane_index,
                    display_title,
                    cwd,
                    current_pid,
                    current_command,
                    process_class,
                    is_shell,
                    is_running,
                    is_interesting,
                    last_activity_at,
                    window_active,
                    pane_active,
                    rank_updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 0, ?17, ?18, ?19, ?20)
                 ON CONFLICT(pane_id) DO UPDATE SET
                    tmux_session_id = excluded.tmux_session_id,
                    tmux_window_id = excluded.tmux_window_id,
                    tmux_pane_id = excluded.tmux_pane_id,
                    session_name = excluded.session_name,
                    window_index = excluded.window_index,
                    window_name = excluded.window_name,
                    pane_index = excluded.pane_index,
                    display_title = excluded.display_title,
                    cwd = excluded.cwd,
                    current_pid = excluded.current_pid,
                    current_command = excluded.current_command,
                    process_class = excluded.process_class,
                    is_shell = excluded.is_shell,
                    is_running = excluded.is_running,
                    last_activity_at = excluded.last_activity_at,
                    window_active = excluded.window_active,
                    pane_active = excluded.pane_active,
                    rank_updated_at = excluded.rank_updated_at",
                params![
                    pane_id,
                    server_id,
                    snapshot.session_id,
                    snapshot.window_id,
                    snapshot.pane_id,
                    snapshot.session_name,
                    snapshot.window_index,
                    snapshot.window_name,
                    snapshot.pane_index,
                    snapshot.pane_title,
                    snapshot.pane_current_path,
                    snapshot.pane_pid,
                    snapshot.pane_current_command,
                    process_class.as_str(),
                    bool_to_sqlite(process_class.is_shell()),
                    bool_to_sqlite(!snapshot.pane_dead),
                    snapshot.window_activity.map(|value| value.to_string()),
                    bool_to_sqlite(snapshot.window_active),
                    bool_to_sqlite(snapshot.pane_active),
                    snapshot.observed_at,
                ],
            )?;
        }

        mark_missing_panes_stale(&tx, server_id, &seen_panes, &observed_at)?;
        tx.commit()?;
        Ok(snapshots.len())
    }

    pub fn list_ranked(&self, server_key: &str, limit: usize) -> Result<Vec<RankedPane>> {
        let limit = i64::try_from(limit).context("ranking limit overflowed i64")?;
        let mut statement = self.conn.prepare(
            "SELECT
                servers.server_key,
                servers.socket_path,
                ps.tmux_session_id,
                ps.tmux_window_id,
                ps.tmux_pane_id,
                ps.session_name,
                ps.window_index,
                ps.window_name,
                ps.pane_index,
                COALESCE(ps.display_title, ''),
                COALESCE(ps.cwd, ''),
                COALESCE(ps.current_command, ''),
                COALESCE(ps.process_class, 'unknown'),
                ps.is_running,
                ps.is_interesting,
                ps.window_active,
                ps.pane_active,
                ps.last_activity_at,
                ps.rank_updated_at
             FROM pane_state ps
             INNER JOIN servers ON servers.id = ps.server_id
             WHERE servers.server_key = ?1
               AND ps.is_running = 1
             ORDER BY
                ps.is_interesting DESC,
                CASE WHEN ps.last_interest_at IS NULL THEN 1 ELSE 0 END,
                ps.last_interest_at DESC,
                CASE WHEN ps.last_signal_at IS NULL THEN 1 ELSE 0 END,
                ps.last_signal_at DESC,
                CASE WHEN ps.last_bell_at IS NULL THEN 1 ELSE 0 END,
                ps.last_bell_at DESC,
                CASE WHEN ps.last_activity_at IS NULL THEN 1 ELSE 0 END,
                CAST(ps.last_activity_at AS INTEGER) DESC,
                ps.window_active DESC,
                ps.pane_active DESC,
                ps.rank_updated_at DESC,
                ps.window_index ASC,
                ps.pane_index ASC
             LIMIT ?2",
        )?;

        let rows = statement.query_map(params![server_key, limit], |row| {
            Ok(RankedPane {
                target: JumpTarget {
                    server_key: row.get(0)?,
                    socket_path: row.get(1)?,
                    session_id: row.get(2)?,
                    window_id: row.get(3)?,
                    pane_id: row.get(4)?,
                },
                session_name: row.get(5)?,
                window_index: row.get(6)?,
                window_name: row.get(7)?,
                pane_index: row.get(8)?,
                display_title: row.get(9)?,
                cwd: row.get(10)?,
                current_command: row.get(11)?,
                process_class: parse_process_class(row.get::<_, String>(12)?),
                is_running: sqlite_to_bool(row.get::<_, i64>(13)?),
                is_interesting: sqlite_to_bool(row.get::<_, i64>(14)?),
                window_active: sqlite_to_bool(row.get::<_, i64>(15)?),
                pane_active: sqlite_to_bool(row.get::<_, i64>(16)?),
                last_activity_at: row.get(17)?,
                rank_updated_at: row.get(18)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn top_ranked(&self, server_key: &str) -> Result<Option<JumpTarget>> {
        Ok(self
            .list_ranked(server_key, 1)?
            .into_iter()
            .next()
            .map(|pane| pane.target))
    }

    pub fn live_pane_count(&self, server_key: &str) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*)
             FROM pane_state ps
             INNER JOIN servers ON servers.id = ps.server_id
             WHERE servers.server_key = ?1 AND ps.is_running = 1",
            params![server_key],
            |row| row.get(0),
        )?;
        usize::try_from(count).map_err(|_| anyhow!("pane count overflowed usize"))
    }

    fn init(&mut self) -> Result<()> {
        self.conn
            .execute_batch(include_str!("../../../sql/0001_init.sql"))?;
        self.conn
            .execute_batch(include_str!("../../../sql/0002_indexes.sql"))?;
        if !servers_table_has_column(&self.conn, "socket_path")?
            || !servers_table_has_column(&self.conn, "start_time")?
        {
            self.conn
                .execute_batch(include_str!("../../../sql/0003_server_identity.sql"))?;
        }
        Ok(())
    }
}

fn prune_stale_servers(
    tx: &rusqlite::Transaction<'_>,
    server_key: &str,
    socket_path: &str,
) -> Result<()> {
    tx.execute(
        "DELETE FROM servers
         WHERE socket_path = ?1
           AND server_key != ?2",
        params![socket_path, server_key],
    )?;
    Ok(())
}

fn mark_missing_panes_stale(
    tx: &rusqlite::Transaction<'_>,
    server_id: i64,
    seen_panes: &[String],
    observed_at: &str,
) -> Result<()> {
    if seen_panes.is_empty() {
        tx.execute(
            "UPDATE panes SET is_alive = 0 WHERE server_id = ?1",
            params![server_id],
        )?;
        tx.execute(
            "UPDATE pane_state
             SET is_running = 0, rank_updated_at = ?2
             WHERE server_id = ?1",
            params![server_id, observed_at],
        )?;
        return Ok(());
    }

    let placeholders = std::iter::repeat_n("?", seen_panes.len())
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "UPDATE panes
         SET is_alive = 0
         WHERE server_id = ?
           AND tmux_pane_id NOT IN ({placeholders})"
    );
    let mut values: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(seen_panes.len() + 1);
    values.push(&server_id);
    for pane in seen_panes {
        values.push(pane);
    }
    tx.execute(&query, values.as_slice())?;

    let query = format!(
        "UPDATE pane_state
         SET is_running = 0, rank_updated_at = ?
         WHERE server_id = ?
           AND tmux_pane_id NOT IN ({placeholders})"
    );
    let mut values: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(seen_panes.len() + 2);
    values.push(&observed_at);
    values.push(&server_id);
    for pane in seen_panes {
        values.push(pane);
    }
    tx.execute(&query, values.as_slice())?;
    Ok(())
}

fn parse_process_class(value: String) -> ProcessClass {
    match value.as_str() {
        "shell" => ProcessClass::Shell,
        "agent" => ProcessClass::Agent,
        "batch" => ProcessClass::Batch,
        "server_watch" => ProcessClass::ServerWatch,
        "interactive_other" => ProcessClass::InteractiveOther,
        _ => ProcessClass::Unknown,
    }
}

fn servers_table_has_column(conn: &Connection, column_name: &str) -> Result<bool> {
    let mut statement = conn.prepare("PRAGMA table_info(servers)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == column_name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn bool_to_sqlite(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn sqlite_to_bool(value: i64) -> bool {
    value != 0
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::Store;
    use tps_core::{PaneSnapshot, server_instance_key};

    static TEST_DB_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn ranking_prefers_recent_window_activity() {
        let db_path = temp_db_path();
        let mut store = Store::open(&db_path).expect("open store");
        store
            .upsert_snapshots(&[
                snapshot("%1", "@1", 0, Some(10), false),
                snapshot("%2", "@2", 1, Some(99), true),
            ])
            .expect("upsert snapshots");

        let ranked = store
            .list_ranked(&test_server_key(), 2)
            .expect("rank panes");
        assert_eq!(ranked[0].target.pane_id, "%2");
        assert!(ranked[0].window_active);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn restart_on_same_socket_gets_fresh_pane_identity_and_prunes_old_server() {
        let db_path = temp_db_path();
        let mut store = Store::open(&db_path).expect("open store");

        store
            .upsert_snapshots(&[snapshot_with_server("%0", "@1", "$1", 0, 111, "100.000Z")])
            .expect("upsert first snapshot");
        store
            .upsert_snapshots(&[snapshot_with_server("%0", "@9", "$9", 9, 222, "200.000Z")])
            .expect("upsert second snapshot");

        let server_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM servers", [], |row| row.get(0))
            .expect("count servers");
        let pane_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM panes", [], |row| row.get(0))
            .expect("count panes");
        let event_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .expect("count events");
        let pane = store
            .conn
            .query_row(
                "SELECT first_seen_at, tmux_session_id, tmux_window_id
                 FROM panes",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .expect("query pane");

        assert_eq!(server_count, 1);
        assert_eq!(pane_count, 1);
        assert_eq!(event_count, 1);
        assert_eq!(pane.0, "200.000Z");
        assert_eq!(pane.1, "$9");
        assert_eq!(pane.2, "@9");

        let _ = std::fs::remove_file(db_path);
    }

    fn snapshot(
        pane_id: &str,
        window_id: &str,
        window_index: i64,
        window_activity: Option<i64>,
        active: bool,
    ) -> PaneSnapshot {
        snapshot_with_server_and_activity(
            pane_id,
            window_id,
            "$1",
            window_index,
            TEST_SERVER_START_TIME,
            "100.000Z",
            window_activity,
            active,
        )
    }

    fn snapshot_with_server(
        pane_id: &str,
        window_id: &str,
        session_id: &str,
        window_index: i64,
        server_start_time: i64,
        observed_at: &str,
    ) -> PaneSnapshot {
        snapshot_with_server_and_activity(
            pane_id,
            window_id,
            session_id,
            window_index,
            server_start_time,
            observed_at,
            Some(10 + window_index),
            false,
        )
    }

    fn snapshot_with_server_and_activity(
        pane_id: &str,
        window_id: &str,
        session_id: &str,
        window_index: i64,
        server_start_time: i64,
        observed_at: &str,
        window_activity: Option<i64>,
        active: bool,
    ) -> PaneSnapshot {
        PaneSnapshot {
            server_key: server_instance_key(TEST_SOCKET_PATH, server_start_time),
            socket_path: TEST_SOCKET_PATH.to_string(),
            server_start_time,
            session_id: session_id.to_string(),
            session_name: "work".to_string(),
            window_id: window_id.to_string(),
            window_index,
            window_name: format!("window-{window_index}"),
            window_activity,
            window_active: active,
            pane_id: pane_id.to_string(),
            pane_index: 0,
            pane_pid: Some(1000 + window_index),
            pane_current_command: "zsh".to_string(),
            pane_current_path: "/tmp".to_string(),
            pane_title: format!("pane-{pane_id}"),
            pane_active: active,
            pane_dead: false,
            observed_at: observed_at.to_string(),
        }
    }

    fn test_server_key() -> String {
        server_instance_key(TEST_SOCKET_PATH, TEST_SERVER_START_TIME)
    }

    const TEST_SOCKET_PATH: &str = "/tmp/tmux.sock";
    const TEST_SERVER_START_TIME: i64 = 111;

    fn temp_db_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos();
        let counter = TEST_DB_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("tmux-pane-switcher-{unique}-{counter}.sqlite"))
    }
}
