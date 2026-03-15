PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS servers (
  id INTEGER PRIMARY KEY,
  server_key TEXT NOT NULL UNIQUE,
  socket_path TEXT NOT NULL,
  start_time INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS panes (
  id INTEGER PRIMARY KEY,
  server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  tmux_pane_id TEXT NOT NULL,
  tmux_window_id TEXT NOT NULL,
  tmux_session_id TEXT NOT NULL,
  session_name TEXT,
  window_name TEXT,
  window_index INTEGER,
  pane_index INTEGER,
  first_seen_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  last_known_title TEXT,
  last_known_cwd TEXT,
  last_known_command TEXT,
  is_alive INTEGER NOT NULL DEFAULT 1,
  UNIQUE(server_id, tmux_pane_id)
);

CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY,
  server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  pane_id INTEGER NOT NULL REFERENCES panes(id) ON DELETE CASCADE,
  event_type TEXT NOT NULL,
  event_ts TEXT NOT NULL,
  pane_pid INTEGER,
  observed_command TEXT,
  process_class TEXT,
  signal_type TEXT,
  signal_terminator TEXT,
  interest_reason TEXT,
  is_interesting INTEGER,
  raw_payload TEXT
);

CREATE TABLE IF NOT EXISTS pane_state (
  pane_id INTEGER PRIMARY KEY REFERENCES panes(id) ON DELETE CASCADE,
  server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  tmux_session_id TEXT NOT NULL,
  tmux_window_id TEXT NOT NULL,
  tmux_pane_id TEXT NOT NULL,
  session_name TEXT,
  window_index INTEGER,
  window_name TEXT,
  pane_index INTEGER,
  display_title TEXT,
  cwd TEXT,
  current_pid INTEGER,
  current_command TEXT,
  process_class TEXT,
  is_shell INTEGER NOT NULL DEFAULT 1,
  is_running INTEGER NOT NULL DEFAULT 1,
  is_interesting INTEGER NOT NULL DEFAULT 0,
  interest_reason TEXT,
  last_output_at TEXT,
  last_bell_at TEXT,
  last_signal_at TEXT,
  last_signal_type TEXT,
  last_activity_at TEXT,
  last_silence_at TEXT,
  last_interest_at TEXT,
  last_process_change_at TEXT,
  window_active INTEGER NOT NULL DEFAULT 0,
  pane_active INTEGER NOT NULL DEFAULT 0,
  rank_updated_at TEXT NOT NULL
);
