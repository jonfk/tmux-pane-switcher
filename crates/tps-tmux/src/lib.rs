use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use tps_core::{PaneSnapshot, is_truthy, server_instance_key};

const SNAPSHOT_FIELDS: [&str; 17] = [
    "session_id",
    "session_name",
    "window_id",
    "window_index",
    "window_name",
    "window_activity",
    "window_active",
    "pane_id",
    "pane_index",
    "pane_pid",
    "pane_current_command",
    "pane_current_path",
    "pane_title",
    "pane_active",
    "pane_dead",
    "socket_path",
    "start_time",
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

    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub fn collect_snapshot(&self) -> Result<Vec<PaneSnapshot>> {
        let format = snapshot_format();
        let output = self.run(["list-panes", "-a", "-F", &format])?;
        let observed_at = iso8601_now();
        let mut snapshots = parse_snapshot_output(&output, &observed_at)?;

        if snapshots
            .iter()
            .any(|snapshot| snapshot.socket_path.is_empty() || snapshot.server_start_time == 0)
        {
            let socket_path = self.socket_path()?;
            let server_start_time = self.server_start_time()?;
            for snapshot in &mut snapshots {
                if snapshot.socket_path.is_empty() {
                    snapshot.socket_path = socket_path.clone();
                }
                if snapshot.server_start_time == 0 {
                    snapshot.server_start_time = server_start_time;
                }
                snapshot.server_key =
                    server_instance_key(&snapshot.socket_path, snapshot.server_start_time);
            }
        }

        Ok(snapshots)
    }

    pub fn server_key(&self) -> Result<String> {
        let socket_path = self.socket_path()?;
        let server_start_time = self.server_start_time()?;
        Ok(server_instance_key(&socket_path, server_start_time))
    }

    pub fn socket_path(&self) -> Result<String> {
        let output = self.run(["display-message", "-p", "#{socket_path}"])?;
        let value = output.trim();
        if value.is_empty() {
            bail!("tmux did not return a socket path for the current server");
        }
        Ok(value.to_string())
    }

    pub fn server_start_time(&self) -> Result<i64> {
        let output = self.run(["display-message", "-p", "#{start_time}"])?;
        let value = output.trim();
        if value.is_empty() {
            bail!("tmux did not return a start time for the current server");
        }
        parse_i64(value, "start_time")
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
        self.run_command(collected)
    }

    fn run_command(&self, collected: Vec<String>) -> Result<String> {
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

fn snapshot_format() -> String {
    SNAPSHOT_FIELDS
        .iter()
        .map(|field| format!("#{{n:{field}}}\t#{{{field}}}"))
        .collect::<Vec<_>>()
        .join("\t")
}

fn parse_snapshot_output(mut output: &str, observed_at: &str) -> Result<Vec<PaneSnapshot>> {
    let mut snapshots = Vec::new();

    while !output.is_empty() {
        let mut fields = Vec::with_capacity(SNAPSHOT_FIELDS.len());
        for index in 0..SNAPSHOT_FIELDS.len() {
            let (length, remainder) = parse_length_prefix(output)?;
            let (value, remainder) = take_bytes(remainder, length)
                .with_context(|| format!("snapshot field {} was shorter than advertised", index))?;
            fields.push(value);
            output = remainder;

            if index + 1 < SNAPSHOT_FIELDS.len() {
                output = output.strip_prefix('\t').ok_or_else(|| {
                    anyhow::anyhow!("missing field separator after field {}", index)
                })?;
            }
        }

        snapshots.push(build_snapshot(&fields, observed_at)?);

        if let Some(remainder) = output.strip_prefix('\n') {
            output = remainder;
        } else if !output.is_empty() {
            bail!("missing row terminator after snapshot row");
        }
    }

    Ok(snapshots)
}

fn parse_length_prefix(input: &str) -> Result<(usize, &str)> {
    let (length, remainder) = input
        .split_once('\t')
        .ok_or_else(|| anyhow::anyhow!("missing length prefix delimiter"))?;
    let length = length
        .parse::<usize>()
        .with_context(|| format!("invalid field length prefix: {length}"))?;
    Ok((length, remainder))
}

fn take_bytes(input: &str, count: usize) -> Option<(&str, &str)> {
    if count > input.len() || !input.is_char_boundary(count) {
        return None;
    }

    Some((&input[..count], &input[count..]))
}

fn build_snapshot(fields: &[&str], observed_at: &str) -> Result<PaneSnapshot> {
    if fields.len() != SNAPSHOT_FIELDS.len() {
        bail!(
            "unexpected list-panes row: expected {} fields, got {}",
            SNAPSHOT_FIELDS.len(),
            fields.len()
        );
    }

    Ok(PaneSnapshot {
        server_key: server_instance_key(
            fields[15],
            parse_optional_i64(fields[16], "start_time")?.unwrap_or_default(),
        ),
        socket_path: fields[15].to_string(),
        server_start_time: parse_optional_i64(fields[16], "start_time")?.unwrap_or_default(),
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
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{TmuxClient, parse_snapshot_output, snapshot_format};
    use tps_core::server_instance_key;

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);
    const TEST_SOCKET_PATH: &str = "/tmp/tmux.sock";
    const TEST_SERVER_START_TIME: &str = "1773537318";

    #[test]
    fn parses_snapshot_rows() {
        let output = prefixed_snapshot_row(&[
            "$1",
            "work",
            "@3",
            "3",
            "editor",
            "1773499610",
            "1",
            "%5",
            "0",
            "4242",
            "zsh",
            "/tmp",
            "pane",
            "1",
            "0",
            TEST_SOCKET_PATH,
            TEST_SERVER_START_TIME,
        ]);
        let snapshot = parse_snapshot_output(&output, "123")
            .expect("parse snapshot")
            .pop()
            .expect("one snapshot");
        assert_eq!(snapshot.session_id, "$1");
        assert_eq!(snapshot.window_activity, Some(1773499610));
        assert!(snapshot.pane_active);
        assert_eq!(snapshot.socket_path, TEST_SOCKET_PATH);
        assert_eq!(snapshot.server_start_time, 1773537318);
        assert_eq!(
            snapshot.server_key,
            server_instance_key(TEST_SOCKET_PATH, 1773537318)
        );
    }

    #[test]
    fn parses_snapshot_rows_with_tabs_and_newlines() {
        let output = prefixed_snapshot_row(&[
            "$1",
            "work",
            "@3",
            "3",
            "edit\\tor",
            "1773499610",
            "1",
            "%5",
            "0",
            "4242",
            "zsh",
            "/tmp/with\ttab\nand-newline",
            "pane\tname\nsecond-line",
            "1",
            "0",
            TEST_SOCKET_PATH,
            TEST_SERVER_START_TIME,
        ]);
        let snapshot = parse_snapshot_output(&output, "123")
            .expect("parse snapshot")
            .pop()
            .expect("one snapshot");

        assert_eq!(snapshot.window_name, "edit\\tor");
        assert_eq!(snapshot.pane_current_path, "/tmp/with\ttab\nand-newline");
        assert_eq!(snapshot.pane_title, "pane\tname\nsecond-line");
    }

    #[test]
    fn parses_snapshot_rows_with_multibyte_utf8() {
        let output = prefixed_snapshot_row(&[
            "$1",
            "projéct",
            "@3",
            "3",
            "editor",
            "1773499610",
            "1",
            "%5",
            "0",
            "4242",
            "zsh",
            "/tmp",
            "✳ Fix organization client tests",
            "1",
            "0",
            TEST_SOCKET_PATH,
            TEST_SERVER_START_TIME,
        ]);
        let snapshot = parse_snapshot_output(&output, "123")
            .expect("parse snapshot")
            .pop()
            .expect("one snapshot");

        assert_eq!(snapshot.session_name, "projéct");
        assert_eq!(snapshot.pane_title, "✳ Fix organization client tests");
    }

    #[test]
    #[cfg(unix)]
    fn collect_snapshot_handles_tabs_and_newlines() {
        let temp_dir = test_temp_dir("snapshot");
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let script_path = temp_dir.join("fake-tmux.sh");
        let format = snapshot_format().replace('\'', "'\"'\"'");
        let output = prefixed_snapshot_row(&[
            "$1",
            "work",
            "@3",
            "3",
            "editor",
            "1773499610",
            "1",
            "%5",
            "0",
            "4242",
            "zsh",
            "/tmp/with\ttab\nline",
            "pane title\nsecond line",
            "1",
            "0",
            TEST_SOCKET_PATH,
            TEST_SERVER_START_TIME,
        ]);
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh
if [ \"$1\" = \"list-panes\" ] && [ \"$2\" = \"-a\" ] && [ \"$3\" = \"-F\" ] && [ \"$4\" = '{format}' ]; then
  cat <<'EOF'
{output}EOF
  exit 0
fi
printf 'unexpected args: %s\\n' \"$*\" >&2
exit 1
"
            ),
        )
        .expect("write fake tmux");

        let mut permissions = fs::metadata(&script_path)
            .expect("stat fake tmux")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("chmod fake tmux");

        let tmux = TmuxClient {
            binary: script_path.to_string_lossy().into_owned(),
        };

        let snapshots = tmux.collect_snapshot().expect("collect snapshot");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].pane_current_path, "/tmp/with\ttab\nline");
        assert_eq!(snapshots[0].pane_title, "pane title\nsecond line");

        fs::remove_dir_all(temp_dir).expect("remove temp dir");
    }

    fn prefixed_snapshot_row(fields: &[&str]) -> String {
        let mut output = String::new();
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                output.push('\t');
            }
            output.push_str(&field.len().to_string());
            output.push('\t');
            output.push_str(field);
        }
        output.push('\n');
        output
    }

    fn test_temp_dir(name: &str) -> std::path::PathBuf {
        let counter = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tps-tmux-test-{name}-{}-{counter}-{nanos}",
            std::process::id()
        ))
    }
}
