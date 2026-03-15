use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tps_core::PaneSnapshot;
use tps_store::Store;

#[test]
fn list_ranked_outputs_valid_tmux_pane_targets() {
    let db_path = temp_db_path();
    let server_key = "/tmp/test-tmux.sock";
    let mut store = Store::open(&db_path).expect("open store");
    store
        .upsert_snapshots(&[
            snapshot(server_key, "%1", "@1", 0, Some(10), false),
            snapshot(server_key, "%2", "@2", 1, Some(99), true),
        ])
        .expect("upsert snapshots");

    let tmux_target_output = run_cli(
        &db_path,
        &[
            "list-ranked",
            "--server-key",
            server_key,
            "--format",
            "tmux-target",
            "--limit",
            "2",
        ],
    );
    assert_eq!(tmux_target_output, "%2\n%1\n");

    let table_output = run_cli(
        &db_path,
        &[
            "list-ranked",
            "--server-key",
            server_key,
            "--format",
            "table",
            "--limit",
            "2",
        ],
    );
    let first_column: Vec<&str> = table_output
        .lines()
        .map(|line| {
            line.split('\t')
                .next()
                .expect("table row has target column")
        })
        .collect();
    assert_eq!(first_column, vec!["%2", "%1"]);

    let jump_target_output = run_cli(
        &db_path,
        &[
            "list-ranked",
            "--server-key",
            server_key,
            "--format",
            "jump-target",
            "--limit",
            "2",
        ],
    );
    assert_eq!(
        jump_target_output,
        "/tmp/test-tmux.sock\t$1\t@2\t%2\n/tmp/test-tmux.sock\t$1\t@1\t%1\n"
    );

    let _ = std::fs::remove_file(db_path);
}

fn run_cli(db_path: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_tmux-pane-switcher"))
        .arg("--db-path")
        .arg(db_path)
        .args(args)
        .output()
        .expect("run tmux-pane-switcher");

    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("stdout is utf-8")
}

fn snapshot(
    server_key: &str,
    pane_id: &str,
    window_id: &str,
    window_index: i64,
    window_activity: Option<i64>,
    active: bool,
) -> PaneSnapshot {
    snapshot_with_ids(SnapshotIds {
        server_key,
        session_id: "$1",
        window_id,
        window_index,
        pane_id,
        pane_index: 0,
        window_activity,
        active,
    })
}

struct SnapshotIds<'a> {
    server_key: &'a str,
    session_id: &'a str,
    window_id: &'a str,
    window_index: i64,
    pane_id: &'a str,
    pane_index: i64,
    window_activity: Option<i64>,
    active: bool,
}

fn snapshot_with_ids(ids: SnapshotIds<'_>) -> PaneSnapshot {
    let SnapshotIds {
        server_key,
        session_id,
        window_id,
        window_index,
        pane_id,
        pane_index,
        window_activity,
        active,
    } = ids;
    PaneSnapshot {
        server_key: server_key.to_string(),
        session_id: session_id.to_string(),
        session_name: "work".to_string(),
        window_id: window_id.to_string(),
        window_index,
        window_name: format!("window-{window_index}"),
        window_activity,
        window_active: active,
        pane_id: pane_id.to_string(),
        pane_index,
        pane_pid: Some(1000 + pane_index),
        pane_current_command: "zsh".to_string(),
        pane_current_path: "/tmp".to_string(),
        pane_title: format!("pane-{pane_id}"),
        pane_active: active,
        pane_dead: false,
        observed_at: "100.000Z".to_string(),
    }
}

fn temp_db_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("tmux-pane-switcher-cli-test-{unique}.sqlite"))
}
