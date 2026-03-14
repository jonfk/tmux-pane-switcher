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

    let _ = std::fs::remove_file(db_path);
}

#[test]
#[cfg(unix)]
fn tmux_target_output_can_be_used_with_select_pane() {
    let db_path = temp_db_path();
    let tmux = TempTmuxServer::start();
    let session_name = "work";
    tmux.new_session(session_name);
    let pane_one = tmux
        .list_panes(session_name)
        .into_iter()
        .next()
        .expect("session has initial pane");
    let pane_two = tmux.split_window(&format!("{session_name}:0"));
    let window_id = tmux.window_id(&pane_one);
    let session_id = tmux.session_id(&pane_one);
    let server_key = tmux.socket_path_string();

    let mut store = Store::open(&db_path).expect("open store");
    store
        .upsert_snapshots(&[
            snapshot_with_ids(
                &server_key,
                &session_id,
                &window_id,
                0,
                &pane_one,
                0,
                Some(10),
                false,
            ),
            snapshot_with_ids(
                &server_key,
                &session_id,
                &window_id,
                0,
                &pane_two,
                1,
                Some(99),
                true,
            ),
        ])
        .expect("upsert snapshots");

    let tmux_target_output = run_cli(
        &db_path,
        &[
            "list-ranked",
            "--server-key",
            &server_key,
            "--format",
            "tmux-target",
            "--limit",
            "1",
        ],
    );
    let target = tmux_target_output.trim();
    assert_eq!(target, pane_two);

    tmux.select_pane(target);
    assert_eq!(tmux.active_pane(&window_id), pane_two);

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
    snapshot_with_ids(
        server_key,
        "$1",
        window_id,
        window_index,
        pane_id,
        0,
        window_activity,
        active,
    )
}

fn snapshot_with_ids(
    server_key: &str,
    session_id: &str,
    window_id: &str,
    window_index: i64,
    pane_id: &str,
    pane_index: i64,
    window_activity: Option<i64>,
    active: bool,
) -> PaneSnapshot {
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

#[cfg(unix)]
struct TempTmuxServer {
    root: PathBuf,
    socket_path: PathBuf,
    config_path: PathBuf,
}

#[cfg(unix)]
impl TempTmuxServer {
    fn start() -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "tpscli-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create temp tmux dir");

        let socket_path = root.join("tmux.sock");
        let config_path = root.join("tmux.conf");
        std::fs::write(&config_path, "").expect("write tmux config");

        let server = Self {
            root,
            socket_path,
            config_path,
        };
        server.run(["start-server"]);
        server
    }

    fn new_session(&self, session_name: &str) {
        self.run([
            "new-session",
            "-d",
            "-s",
            session_name,
            "-x",
            "120",
            "-y",
            "40",
        ]);
    }

    fn split_window(&self, target: &str) -> String {
        self.run(["split-window", "-d", "-P", "-F", "#{pane_id}", "-t", target])
            .trim()
            .to_string()
    }

    fn list_panes(&self, target: &str) -> Vec<String> {
        self.run(["list-panes", "-t", target, "-F", "#{pane_id}"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn window_id(&self, target: &str) -> String {
        self.run(["display-message", "-p", "-t", target, "#{window_id}"])
            .trim()
            .to_string()
    }

    fn session_id(&self, target: &str) -> String {
        self.run(["display-message", "-p", "-t", target, "#{session_id}"])
            .trim()
            .to_string()
    }

    fn select_pane(&self, target: &str) {
        self.run(["select-pane", "-t", target]);
    }

    fn active_pane(&self, target: &str) -> String {
        self.run([
            "list-panes",
            "-t",
            target,
            "-F",
            "#{pane_id} #{pane_active}",
        ])
        .lines()
        .find_map(|line| {
            let (pane_id, active) = line.split_once(' ')?;
            if active == "1" {
                Some(pane_id.to_string())
            } else {
                None
            }
        })
        .expect("window has active pane")
    }

    fn socket_path_string(&self) -> String {
        self.socket_path.to_string_lossy().into_owned()
    }

    fn run<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new("tmux")
            .arg("-f")
            .arg(&self.config_path)
            .arg("-S")
            .arg(&self.socket_path)
            .args(args)
            .output()
            .expect("run tmux command");

        assert!(
            output.status.success(),
            "tmux command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8(output.stdout).expect("tmux stdout is utf-8")
    }
}

#[cfg(unix)]
impl Drop for TempTmuxServer {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .arg("-f")
            .arg(&self.config_path)
            .arg("-S")
            .arg(&self.socket_path)
            .args(["kill-server"])
            .output();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
