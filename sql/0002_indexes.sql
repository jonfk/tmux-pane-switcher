CREATE INDEX IF NOT EXISTS idx_events_pane_ts ON events(pane_id, event_ts DESC);
CREATE INDEX IF NOT EXISTS idx_events_type_ts ON events(event_type, event_ts DESC);
CREATE INDEX IF NOT EXISTS idx_servers_socket_path ON servers(socket_path);
CREATE INDEX IF NOT EXISTS idx_pane_state_rank
ON pane_state(
  server_id,
  is_interesting DESC,
  last_interest_at DESC,
  last_signal_at DESC,
  last_bell_at DESC,
  last_activity_at DESC,
  window_active DESC,
  pane_active DESC,
  rank_updated_at DESC
);
