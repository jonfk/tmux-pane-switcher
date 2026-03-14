# tmux-pane-switcher Plan

## Goals

- Implement a tmux plugin that helps the user jump back to panes with recent interesting activity or likely state changes
- Track panes as the primary unit of state
- Support jumping directly to panes across windows and tmux sessions within the same tmux server
- Prefer zero-integration observation in v1, meaning no required shell hooks and no required changes to programs running inside panes
- Implement the core program in Rust and manage its installation separately from TPM
- Persist state in SQLite for ranking, history, and recovery across detach and reattach

## Recommended Architecture

Use an observer-based architecture for v1:

- A thin tmux plugin layer in shell for TPM compatibility, key bindings, and process supervision
- A separately installed Rust CLI as the core program
- A long-lived Rust observer that attaches to tmux in control mode and observes pane output and metadata changes
- Rust-owned process inspection and heuristic evaluation using tmux metadata plus OS process information
- SQLite, managed by the Rust CLI, as the source of truth for event history and current pane state

This is a better fit than a hook-first design because it avoids depending on shell integration, keeps the TPM side minimal, and gives a stronger foundation for state management, process inspection, and ranking logic.

## Core Product Decisions

- Primary tracked unit: pane
- Secondary view: window, derived from the highest-ranked pane in that window
- Default operating mode: zero-integration
- Packaging model:
  - TPM installs only shell wrappers and tmux bindings
  - The Rust CLI is expected to already be installed and available on `PATH`
- Supported signal families:
  - tmux control-mode output notifications such as `%output`
  - tmux pane metadata such as `pane_current_command`, `pane_pid`, `pane_tty`, `pane_title`, and pane location identifiers
  - tmux-native alerts such as bell, activity, and silence when available
  - OS process inspection for the pane's foreground process tree
- Persistence model:
  - append-only event log
  - materialized current pane state table for fast ranking and jumps

## Non-Goals and Constraints

- v1 should not depend on zsh or bash hooks
- v1 should not require programs such as Codex, Claude, test runners, or dev servers to emit custom signals
- v1 should not assume tmux can observe arbitrary macOS Notification Center notifications triggered by programs
- v1 TPM installation should not be responsible for compiling or installing the Rust core binary
- tmux-observable signals include pane output, bells, and tmux alerts; arbitrary desktop notifications do not appear to be a reliable tmux signal source

Optional explicit program integration may still be added later as a higher-confidence hinting layer, but it should not be required for the base product.

## Distribution Model

Separate distribution concerns between the tmux plugin and the core executable:

- The TPM-managed side should install a shell wrapper, tmux bindings, and minimal configuration glue
- The wrapper should call the Rust CLI and fail clearly if the CLI is not installed
- The Rust CLI should own the observer, SQLite schema and migrations, ranking logic, process inspection, and jump commands
- The Rust CLI can be installed through a separate mechanism such as `cargo install`, a package manager, or prebuilt release artifacts

This keeps TPM simple and avoids turning plugin installation into language-runtime or build-toolchain management.

## Jumping Model

To jump to a pane, the plugin must retain:

- tmux server identity
- session id
- window id
- pane id

Jump flow:

1. Switch client to the pane's session.
2. Select the pane's window.
3. Select the target pane.

This works across windows and tmux sessions as long as the tmux server is still alive. Pane IDs do not survive a tmux server restart, so persisted state after a restart is useful for history but not for direct jumps to the original pane.

## Observer Model

The observer should run as a long-lived tmux control-mode client and continuously ingest:

- `%output` notifications to detect live pane output
- pane lifecycle changes such as pane death or layout changes
- pane metadata snapshots for all panes on startup and periodically during reconciliation
- tmux alert state where available

The observer should maintain a live per-pane model with:

- pane identity and location
- pane title and current path where available
- current process classification
- whether the pane is running a shell or a non-shell foreground process
- last output time
- last bell or tmux alert time
- last inferred "interesting" transition time
- the reason the pane became interesting

## Program Classification

The plugin should classify panes by process tree, not only by `pane_current_command`.

Suggested initial classes:

- `shell`: `zsh`, `bash`, `fish`, and similar interactive shells
- `agent`: coding-agent CLIs such as Codex or Claude
- `batch`: compilers, test runners, one-shot scripts, and build tools
- `server_watch`: dev servers, watchers, logs, and tail-like long-lived producers
- `interactive_other`: editors, TUIs, SSH sessions, database consoles, and similar interactive programs
- `unknown`: anything not yet recognized

Inputs for classification:

- tmux `pane_current_command`
- tmux `pane_pid`
- pane TTY
- OS process tree inspection rooted at the pane PID or current foreground process group

`pane_current_command` is useful but not sufficient on its own, since it may not always reflect the most relevant child process currently active in the pane.

## Heuristic Model

The product should be framed around "interesting panes" rather than "notified panes".

A pane becomes interesting when one or more of these heuristics fire:

- A pane is running a non-shell foreground process that is likely user-relevant
- A pane in class `agent` or `batch` produces output and then goes idle past a configured threshold
- A pane receives a bell or a tmux alert
- A pane running a non-shell process returns to a shell, suggesting that work has completed
- A pane changes from one classified process type to another in a way that suggests a new task started

Suggested class-specific behavior:

- `agent`: interesting on output bursts, on idle-after-output, and on process exit back to shell
- `batch`: interesting on start, on failure signals if detectable, on idle-after-output, and on process exit back to shell
- `server_watch`: interesting on bell or explicit alert, but not merely because output stops
- `interactive_other`: interesting while active if the process is not a shell, but generally lower priority than `agent` and `batch`
- `shell`: usually not interesting unless paired with a bell or another alert signal

This avoids the biggest false positive in a generic "silence means done" model: long-lived quiet processes such as servers or SSH sessions that may stop output without having completed.

## Event Model

All observer writes should emit a common JSON payload shape to a single recorder interface implemented inside the Rust CLI.

Example payload:

```json
{
  "schema_version": 1,
  "event_type": "heuristic_transition",
  "event_ts": "2026-03-13T20:14:52.123Z",
  "server_id": "/tmp/tmux-501/default",
  "session_id": "$1",
  "session_name": "work",
  "window_id": "@7",
  "window_index": 2,
  "pane_id": "%11",
  "pane_index": 1,
  "pane_tty": "/dev/ttys012",
  "pane_title": "editor",
  "cwd": "/Users/jfokkan/Developer/jonfk_code/tmux-pane-switcher",
  "pane_pid": 18310,
  "observed_command": "codex",
  "process_class": "agent",
  "interest_reason": "idle_after_output",
  "is_interesting": true,
  "raw_payload": null
}
```

### Required Fields

- `schema_version`
- `event_type`
- `event_ts`
- `server_id`
- `session_id`
- `window_id`
- `pane_id`

### Event Types

- `pane_snapshot`
- `pane_output`
- `pane_alert`
- `process_classified`
- `heuristic_transition`
- `pane_exit`

### Event-Specific Fields

Observer events may populate:

- `pane_tty`
- `pane_title`
- `cwd`
- `pane_pid`
- `observed_command`
- `process_class`
- `interest_reason`
- `is_interesting`
- `raw_payload`

## SQLite Data Model

Use SQLite with two layers:

- `events`: append-only event history
- `pane_state`: current materialized pane state for fast ranking and jumps

### Schema

```sql
PRAGMA foreign_keys = ON;

CREATE TABLE servers (
  id INTEGER PRIMARY KEY,
  server_key TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL
);

CREATE TABLE panes (
  id INTEGER PRIMARY KEY,
  server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  tmux_pane_id TEXT NOT NULL,
  tmux_window_id TEXT NOT NULL,
  tmux_session_id TEXT NOT NULL,
  session_name TEXT,
  window_index INTEGER,
  pane_index INTEGER,
  pane_tty TEXT,
  first_seen_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  last_known_title TEXT,
  last_known_cwd TEXT,
  last_known_command TEXT,
  is_alive INTEGER NOT NULL DEFAULT 1,
  UNIQUE(server_id, tmux_pane_id)
);

CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  pane_id INTEGER NOT NULL REFERENCES panes(id) ON DELETE CASCADE,
  event_type TEXT NOT NULL,
  event_ts TEXT NOT NULL,
  pane_pid INTEGER,
  observed_command TEXT,
  process_class TEXT,
  interest_reason TEXT,
  is_interesting INTEGER,
  raw_payload TEXT
);

CREATE INDEX idx_events_pane_ts ON events(pane_id, event_ts DESC);
CREATE INDEX idx_events_type_ts ON events(event_type, event_ts DESC);

CREATE TABLE pane_state (
  pane_id INTEGER PRIMARY KEY REFERENCES panes(id) ON DELETE CASCADE,
  server_id INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  tmux_session_id TEXT NOT NULL,
  tmux_window_id TEXT NOT NULL,
  tmux_pane_id TEXT NOT NULL,
  session_name TEXT,
  window_index INTEGER,
  pane_index INTEGER,
  display_title TEXT,
  cwd TEXT,
  pane_tty TEXT,
  current_pid INTEGER,
  current_command TEXT,
  process_class TEXT,
  is_shell INTEGER NOT NULL DEFAULT 1,
  is_running INTEGER NOT NULL DEFAULT 1,
  is_interesting INTEGER NOT NULL DEFAULT 0,
  interest_reason TEXT,
  last_output_at TEXT,
  last_alert_at TEXT,
  last_interest_at TEXT,
  last_process_change_at TEXT,
  rank_updated_at TEXT NOT NULL
);

CREATE INDEX idx_pane_state_rank
ON pane_state(
  server_id,
  is_interesting DESC,
  last_interest_at DESC,
  last_alert_at DESC,
  last_output_at DESC
);
```

## State Update Rules

### On `pane_snapshot`

- Upsert `servers`
- Upsert `panes`
- Refresh pane identity, path, title, pid, command, and liveness in `pane_state`
- If process classification changed, emit `process_classified`

### On `pane_output`

- Insert an `events` row
- Update `pane_state.last_output_at`
- Re-evaluate the pane heuristics

### On `pane_alert`

- Insert an `events` row
- Update `pane_state.last_alert_at`
- Mark the pane interesting with an alert-related `interest_reason`

### On `process_classified`

- Insert an `events` row
- Update `pane_state.current_command`, `current_pid`, `process_class`, and `is_shell`
- Update `last_process_change_at`
- Re-evaluate whether the pane should become interesting

### On `heuristic_transition`

- Insert an `events` row
- Update `pane_state.is_interesting`
- Update `pane_state.interest_reason`
- If the pane became interesting, set `last_interest_at = event_ts`

### On `pane_exit`

- Insert an `events` row
- Mark `panes.is_alive = 0`
- Mark `pane_state.is_running = 0`
- Either remove `pane_state` immediately or defer cleanup to a reconciler

## Ranking Model

Initial ranking should prefer:

1. Panes that are currently interesting
2. Most recently interesting panes
3. Panes with recent bells or tmux alerts
4. Panes with recent output
5. Other non-dead panes as a fallback

Example query:

```sql
SELECT
  ps.tmux_session_id,
  ps.tmux_window_id,
  ps.tmux_pane_id
FROM pane_state ps
WHERE ps.server_id = ?
  AND ps.is_running = 1
ORDER BY
  ps.is_interesting DESC,
  ps.last_interest_at DESC NULLS LAST,
  ps.last_alert_at DESC NULLS LAST,
  ps.last_output_at DESC NULLS LAST
LIMIT 1;
```

This should support MRU-style switching through panes that have recently become relevant to the user.

## Proposed CLI Surface

Keep tmux integration thin by routing all writes and reads through one Rust CLI surface.

Commands:

- `observe`
- `snapshot-panes`
- `record-output`
- `record-alert`
- `classify-pane`
- `evaluate-heuristics`
- `list-ranked`
- `jump-top`
- `jump-next`
- `cleanup-stale`

The TPM wrapper script should call these commands and should not duplicate application logic.

## Proposed Repository Layout

```text
tmux-pane-switcher.tmux
scripts/
  pane-switcher-wrapper
rust/
  tmux-pane-switcher/
    Cargo.toml
    src/
sql/
  schema.sql
  queries.sql
docs/
  installation.md
  configuration.md
  heuristics.md
test/
  fixtures/
```

## Implementation Phases

### Phase 1: Plugin Skeleton and Core Persistence

- Add TPM-compatible `tmux-pane-switcher.tmux`
- Add a shell wrapper that locates and invokes the Rust CLI
- Add a Rust crate for the core CLI
- Add SQLite schema and initialization path owned by the Rust CLI
- Add Rust CLI commands for:
  - schema initialization
  - manual pane snapshot recording
  - ranking query
  - jump-to-pane query output
- Add minimal tmux command bindings

Acceptance criteria:

- The wrapper can invoke the installed Rust CLI
- The Rust CLI initializes its database
- A manually inserted pane snapshot updates pane state
- The plugin can query the top-ranked pane target

### Phase 2: Control-Mode Observer

- Add a long-lived Rust observer process that attaches to tmux in control mode
- Ingest `%output` and related pane notifications
- Reconcile pane metadata on startup and periodically
- Persist output activity and pane lifecycle changes

Acceptance criteria:

- Pane output updates `last_output_at`
- New and dead panes are reflected in state
- Observer restarts can reconcile current panes without corrupting history

### Phase 3: Process Classification

- Inspect pane processes using tmux metadata plus OS process information
- Add a first-pass classifier for `shell`, `agent`, `batch`, `server_watch`, `interactive_other`, and `unknown`
- Persist process class changes and shell versus non-shell state

Acceptance criteria:

- The classifier distinguishes shells from non-shell panes
- At least known agent and batch examples classify correctly in tests or fixtures
- Process class changes trigger heuristic re-evaluation

### Phase 4: Heuristics and Ranking

- Implement class-specific heuristics for when a pane becomes interesting
- Add idle-after-output thresholds where appropriate
- Rank panes by `is_interesting`, `last_interest_at`, alert recency, and output recency

Acceptance criteria:

- Agent and batch panes become interesting after likely completion or meaningful state changes
- Server and watch panes do not become false positives just because output stops
- Ranked queries match the intended MRU behavior

### Phase 5: Jump and Selection UX

- Add `jump-top`
- Add cycling or menu-based pane selection
- Optionally derive a window-oriented view from pane rankings

Acceptance criteria:

- The plugin can jump across windows and tmux sessions
- The selected pane becomes active in the client

### Phase 6: Reconciliation and Cleanup

- Detect dead panes and stale state
- Handle tmux server restarts gracefully
- Keep history while invalidating unusable jump targets

Acceptance criteria:

- Stale panes are not offered as jump targets
- Historical events remain queryable after cleanup

### Phase 7: Documentation and Packaging

- Document TPM installation
- Document Rust CLI installation separately from TPM installation
- Document observer lifecycle and expected limitations
- Document configuration options and heuristic tuning

Acceptance criteria:

- A new user can install the Rust CLI, install the TPM wrapper, and run the observer from docs alone
- The docs clearly explain the difference between tmux-observable alerts and arbitrary OS notifications
- The docs clearly explain that TPM does not install the Rust binary

## Open Questions

- How should the process inspector determine the most relevant foreground process for a pane on macOS?
- Which programs should be treated as first-class examples in v1: coding agents, tests and builds, dev servers, SSH sessions, editors, or something else?
- Should failures be ranked above successful completions when they can be detected?
- Should stale pane cleanup happen lazily during reads or eagerly via explicit reconciliation?
- At what threshold should `agent` and `batch` panes be considered idle-after-output?
- Should a future version support optional explicit hints from known tools for better accuracy?

## Recommended Next Step

Write a Phase 1 and Phase 2 implementation spec with:

- exact tmux control-mode commands and observer lifecycle
- exact Rust CLI command names and wrapper invocation contract
- exact SQLite initialization path
- exact recorder CLI contract inside the Rust program
- the initial classification table and heuristic rules
- acceptance tests for pane snapshots, output ingestion, ranking, and pane jumps
