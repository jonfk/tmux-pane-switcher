use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessClass {
    Shell,
    Agent,
    Batch,
    ServerWatch,
    InteractiveOther,
    Unknown,
}

impl ProcessClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Agent => "agent",
            Self::Batch => "batch",
            Self::ServerWatch => "server_watch",
            Self::InteractiveOther => "interactive_other",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_shell(self) -> bool {
        matches!(self, Self::Shell)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub server_key: String,
    pub socket_path: String,
    pub server_start_time: i64,
    pub session_id: String,
    pub session_name: String,
    pub window_id: String,
    pub window_index: i64,
    pub window_name: String,
    pub window_activity: Option<i64>,
    pub window_active: bool,
    pub pane_id: String,
    pub pane_index: i64,
    pub pane_pid: Option<i64>,
    pub pane_current_command: String,
    pub pane_current_path: String,
    pub pane_title: String,
    pub pane_active: bool,
    pub pane_dead: bool,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JumpTarget {
    pub server_key: String,
    pub socket_path: String,
    pub session_id: String,
    pub window_id: String,
    pub pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankedPane {
    pub target: JumpTarget,
    pub session_name: String,
    pub window_index: i64,
    pub window_name: String,
    pub pane_index: i64,
    pub display_title: String,
    pub cwd: String,
    pub current_command: String,
    pub process_class: ProcessClass,
    pub is_running: bool,
    pub is_interesting: bool,
    pub window_active: bool,
    pub pane_active: bool,
    pub last_activity_at: Option<String>,
    pub rank_updated_at: String,
}

impl RankedPane {
    pub fn tmux_target(&self) -> String {
        self.target.pane_id.clone()
    }
}

pub fn is_truthy(value: &str) -> bool {
    matches!(value.trim(), "1" | "true" | "yes" | "on")
}

pub fn server_instance_key(socket_path: &str, server_start_time: i64) -> String {
    format!("{socket_path}::{server_start_time}")
}

pub fn classify_command(command: &str) -> ProcessClass {
    let normalized = command.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return ProcessClass::Unknown;
    }

    let shell_commands = [
        "sh", "bash", "zsh", "fish", "nu", "ksh", "dash", "ash", "tcsh", "csh",
    ];
    if shell_commands.contains(&normalized.as_str()) {
        return ProcessClass::Shell;
    }

    let agent_commands = ["codex", "claude", "aider", "opencode", "cursor-agent"];
    if agent_commands.contains(&normalized.as_str()) {
        return ProcessClass::Agent;
    }

    let batch_commands = [
        "cargo", "make", "just", "pytest", "go", "npm", "pnpm", "yarn", "node", "python",
        "python3", "uv", "tox", "bundle", "rake",
    ];
    if batch_commands.contains(&normalized.as_str()) {
        return ProcessClass::Batch;
    }

    let server_watch_commands = [
        "tail", "watch", "webpack", "vite", "next", "rails", "docker", "kubectl",
    ];
    if server_watch_commands.contains(&normalized.as_str()) {
        return ProcessClass::ServerWatch;
    }

    ProcessClass::InteractiveOther
}

#[cfg(test)]
mod tests {
    use super::{ProcessClass, classify_command, is_truthy};

    #[test]
    fn classifies_shells() {
        assert_eq!(classify_command("zsh"), ProcessClass::Shell);
    }

    #[test]
    fn classifies_agents() {
        assert_eq!(classify_command("codex"), ProcessClass::Agent);
    }

    #[test]
    fn parses_tmux_booleans() {
        assert!(is_truthy("1"));
        assert!(!is_truthy("0"));
    }
}
