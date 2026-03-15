#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tps_core::server_instance_key;
use tps_tmux::TmuxClient;

#[test]
fn collect_snapshot_round_trips_real_tmux_fields() {
    let server = TempTmuxServer::start();
    let newline_dir = server.root.join("with\nline");
    fs::create_dir_all(&newline_dir).expect("create newline dir");

    server.new_session("projéct", server.shell(), &newline_dir);

    let pane_id = server.display("#{pane_id}", "projéct:0.0");
    server.rename_window("projéct:0", "win\tname");
    server.set_pane_title(&pane_id, "✳ Fix organization client tests");

    let wrapper = server.tmux_wrapper();
    let tmux = TmuxClient::with_binary(wrapper.to_string_lossy().into_owned());
    let snapshots = tmux.collect_snapshot().expect("collect snapshot");
    let canonical_newline_dir = fs::canonicalize(&newline_dir).expect("canonicalize newline dir");

    assert_eq!(snapshots.len(), 1);
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.session_name, "projéct");
    assert_eq!(snapshot.window_name, "win\\tname");
    assert_eq!(
        snapshot.pane_current_path,
        canonical_newline_dir.to_string_lossy()
    );
    assert_eq!(snapshot.pane_title, "✳ Fix organization client tests");
    assert_eq!(snapshot.socket_path, server.socket_path_string());
    assert_eq!(snapshot.server_start_time, server.start_time());
    assert_eq!(
        snapshot.server_key,
        server_instance_key(&server.socket_path_string(), server.start_time())
    );
    assert_eq!(snapshot.pane_id, pane_id);
}

struct TempTmuxServer {
    root: PathBuf,
    socket_path: PathBuf,
    config_path: PathBuf,
    wrapper_path: PathBuf,
    tmux_bin: String,
}

impl TempTmuxServer {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tps-tmux-integration-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create temp dir");

        let socket_path = root.join("tmux.sock");
        let config_path = root.join("tmux.conf");
        let wrapper_path = root.join("tmux-under-test.sh");
        let tmux_bin = std::env::var("TMUX_BIN").unwrap_or_else(|_| "tmux".to_string());

        fs::write(&config_path, "").expect("write config");
        fs::write(
            &wrapper_path,
            format!(
                "#!/bin/sh\nexec {} -f {} -S {} \"$@\"\n",
                sh_quote(&tmux_bin),
                sh_quote_path(&config_path),
                sh_quote_path(&socket_path)
            ),
        )
        .expect("write wrapper");

        let mut permissions = fs::metadata(&wrapper_path)
            .expect("stat wrapper")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrapper_path, permissions).expect("chmod wrapper");

        let server = Self {
            root,
            socket_path,
            config_path,
            wrapper_path,
            tmux_bin,
        };
        server.run(["start-server"]);
        server
    }

    fn shell(&self) -> String {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }

    fn tmux_wrapper(&self) -> &Path {
        &self.wrapper_path
    }

    fn socket_path_string(&self) -> String {
        self.socket_path.to_string_lossy().into_owned()
    }

    fn start_time(&self) -> i64 {
        self.display("#{start_time}", "projéct:0.0")
            .parse()
            .expect("start time is an integer")
    }

    fn new_session(&self, name: &str, shell: String, start_directory: &Path) {
        self.run([
            "new-session",
            "-d",
            "-s",
            name,
            "-x",
            "120",
            "-y",
            "40",
            "-c",
            &start_directory.to_string_lossy(),
            &shell,
        ]);
    }

    fn rename_window(&self, target: &str, name: &str) {
        self.run(["rename-window", "-t", target, name]);
    }

    fn set_pane_title(&self, target: &str, title: &str) {
        self.run(["select-pane", "-t", target, "-T", title]);
    }

    fn display(&self, format: &str, target: &str) -> String {
        self.run(["display-message", "-p", "-t", target, format])
            .trim()
            .to_string()
    }

    fn run<const N: usize>(&self, args: [&str; N]) -> String {
        let output = Command::new(&self.tmux_bin)
            .arg("-f")
            .arg(&self.config_path)
            .arg("-S")
            .arg(&self.socket_path)
            .args(args)
            .output()
            .expect("run tmux");

        assert!(
            output.status.success(),
            "tmux command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8(output.stdout).expect("tmux stdout is utf-8")
    }
}

impl Drop for TempTmuxServer {
    fn drop(&mut self) {
        let _ = Command::new(&self.tmux_bin)
            .arg("-f")
            .arg(&self.config_path)
            .arg("-S")
            .arg(&self.socket_path)
            .args(["kill-server"])
            .output();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn sh_quote_path(path: &Path) -> String {
    sh_quote(&path.to_string_lossy())
}
