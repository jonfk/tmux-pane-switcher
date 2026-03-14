use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use tps_core::{JumpTarget, PaneSnapshot, is_truthy};

const SNAPSHOT_FIELDS: [&str; 16] = [
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
        let format = snapshot_format();
        let output = self.run(["list-panes", "-a", "-F", &format])?;
        let observed_at = iso8601_now();
        let mut snapshots = parse_snapshot_output(&output, &observed_at)?;

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
        self.run_on_server(
            &target.server_key,
            ["switch-client", "-t", &target.session_id],
        )?;
        self.run_on_server(
            &target.server_key,
            ["select-window", "-t", &target.window_id],
        )?;
        self.run_on_server(&target.server_key, ["select-pane", "-t", &target.pane_id])?;
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
        self.run_command(collected)
    }

    fn run_on_server<I, S>(&self, server_key: &str, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut collected = vec!["-S".to_string(), server_key.to_string()];
        collected.extend(args.into_iter().map(|arg| arg.as_ref().to_string()));
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

fn parse_snapshot_output<'a>(mut output: &'a str, observed_at: &str) -> Result<Vec<PaneSnapshot>> {
    let mut snapshots = Vec::new();

    while !output.is_empty() {
        let mut fields = Vec::with_capacity(SNAPSHOT_FIELDS.len());
        for index in 0..SNAPSHOT_FIELDS.len() {
            let (length, remainder) = parse_length_prefix(output)?;
            let (value, remainder) = take_chars(remainder, length)
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

fn take_chars(input: &str, count: usize) -> Option<(&str, &str)> {
    if count == 0 {
        return Some(("", input));
    }

    let mut end = None;
    for (seen, (index, ch)) in input.char_indices().enumerate() {
        if seen + 1 == count {
            end = Some(index + ch.len_utf8());
            break;
        }
    }

    let end = end?;
    Some((&input[..end], &input[end..]))
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
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    use tps_core::JumpTarget;

    use super::{TmuxClient, parse_snapshot_output, snapshot_format};

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

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
            "/tmp/tmux.sock",
        ]);
        let snapshot = parse_snapshot_output(&output, "123")
            .expect("parse snapshot")
            .pop()
            .expect("one snapshot");
        assert_eq!(snapshot.session_id, "$1");
        assert_eq!(snapshot.window_activity, Some(1773499610));
        assert!(snapshot.pane_active);
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
            "/tmp/tmux.sock",
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
    #[cfg(unix)]
    fn jump_uses_target_server_socket() {
        let temp_dir = test_temp_dir("jump");
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let log_path = temp_dir.join("tmux.log");
        let script_path = temp_dir.join("fake-tmux.sh");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n",
                log_path.display()
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
        let target = JumpTarget {
            server_key: "/tmp/custom.sock".to_string(),
            session_id: "$1".to_string(),
            window_id: "@2".to_string(),
            pane_id: "%3".to_string(),
        };

        tmux.jump(&target).expect("jump succeeds");

        let logged = fs::read_to_string(&log_path).expect("read log");
        let args: Vec<&str> = logged.lines().collect();
        assert_eq!(
            args,
            vec![
                "-S",
                "/tmp/custom.sock",
                "switch-client",
                "-t",
                "$1",
                "-S",
                "/tmp/custom.sock",
                "select-window",
                "-t",
                "@2",
                "-S",
                "/tmp/custom.sock",
                "select-pane",
                "-t",
                "%3",
            ]
        );

        fs::remove_dir_all(temp_dir).expect("remove temp dir");
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
            "/tmp/tmux.sock",
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
            output.push_str(&field.chars().count().to_string());
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
