use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use tps_core::{JumpTarget, PaneSnapshot, is_truthy};

const SNAPSHOT_FIELDS: [&str; 16] = [
    "#{session_id}",
    "#{session_name}",
    "#{window_id}",
    "#{window_index}",
    "#{window_name}",
    "#{window_activity}",
    "#{window_active}",
    "#{pane_id}",
    "#{pane_index}",
    "#{pane_pid}",
    "#{pane_current_command}",
    "#{pane_current_path}",
    "#{pane_title}",
    "#{pane_active}",
    "#{pane_dead}",
    "#{socket_path}",
];

#[derive(Debug, Default, Clone)]
pub struct TmuxClient {
    binary: String,
}

impl TmuxClient {
    pub fn new() -> Self {
        Self {
            binary: std::env::var("TMUX_BIN").unwrap_or_else(|_| "tmux".to_string()),
        }
    }

    pub fn collect_snapshot(&self) -> Result<Vec<PaneSnapshot>> {
        let output = self.run(["list-panes", "-a", "-F", &SNAPSHOT_FIELDS.join("\t")])?;
        let observed_at = iso8601_now();
        let mut snapshots = Vec::new();

        for line in output.lines().filter(|line| !line.trim().is_empty()) {
            snapshots.push(parse_snapshot_line(line, &observed_at)?);
        }

        if snapshots
            .iter()
            .any(|snapshot| snapshot.server_key.is_empty())
        {
            let server_key = self.server_key()?;
            for snapshot in &mut snapshots {
                if snapshot.server_key.is_empty() {
                    snapshot.server_key = server_key.clone();
                }
            }
        }

        Ok(snapshots)
    }

    pub fn server_key(&self) -> Result<String> {
        let output = self.run(["display-message", "-p", "#{socket_path}"])?;
        let value = output.trim();
        if value.is_empty() {
            bail!("tmux did not return a socket path for the current server");
        }
        Ok(value.to_string())
    }

    pub fn jump(&self, target: &JumpTarget) -> Result<()> {
        self.run(["switch-client", "-t", &target.session_id])?;
        self.run(["select-window", "-t", &target.window_id])?;
        self.run(["select-pane", "-t", &target.pane_id])?;
        Ok(())
    }

    pub fn check_tmux(&self) -> Result<String> {
        self.run(["-V"]).map(|value| value.trim().to_string())
    }

    fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let collected: Vec<String> = args
            .into_iter()
            .map(|arg| arg.as_ref().to_string())
            .collect();
        let output = Command::new(&self.binary)
            .args(&collected)
            .output()
            .with_context(|| format!("failed to run `{}`", self.binary))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "tmux command failed ({}): {}",
                collected.join(" "),
                stderr.trim().if_empty(stdout.trim())
            );
        }

        String::from_utf8(output.stdout).context("tmux output was not valid UTF-8")
    }
}

fn parse_snapshot_line(line: &str, observed_at: &str) -> Result<PaneSnapshot> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() != SNAPSHOT_FIELDS.len() {
        bail!(
            "unexpected list-panes row: expected {} fields, got {}",
            SNAPSHOT_FIELDS.len(),
            fields.len()
        );
    }

    Ok(PaneSnapshot {
        session_id: fields[0].to_string(),
        session_name: fields[1].to_string(),
        window_id: fields[2].to_string(),
        window_index: parse_i64(fields[3], "window_index")?,
        window_name: fields[4].to_string(),
        window_activity: parse_optional_i64(fields[5], "window_activity")?,
        window_active: is_truthy(fields[6]),
        pane_id: fields[7].to_string(),
        pane_index: parse_i64(fields[8], "pane_index")?,
        pane_pid: parse_optional_i64(fields[9], "pane_pid")?,
        pane_current_command: fields[10].to_string(),
        pane_current_path: fields[11].to_string(),
        pane_title: fields[12].to_string(),
        pane_active: is_truthy(fields[13]),
        pane_dead: is_truthy(fields[14]),
        server_key: fields[15].to_string(),
        observed_at: observed_at.to_string(),
    })
}

fn parse_i64(value: &str, field: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .with_context(|| format!("invalid integer for {field}: {value}"))
}

fn parse_optional_i64(value: &str, field: &str) -> Result<Option<i64>> {
    if value.trim().is_empty() {
        return Ok(None);
    }

    parse_i64(value, field).map(Some)
}

fn iso8601_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}

trait EmptyFallback {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl EmptyFallback for str {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_snapshot_line;

    #[test]
    fn parses_snapshot_rows() {
        let line = "$1\twork\t@3\t3\teditor\t1773499610\t1\t%5\t0\t4242\tzsh\t/tmp\tpane\t1\t0\t/tmp/tmux.sock";
        let snapshot = parse_snapshot_line(line, "123").expect("parse snapshot");
        assert_eq!(snapshot.session_id, "$1");
        assert_eq!(snapshot.window_activity, Some(1773499610));
        assert!(snapshot.pane_active);
    }
}
