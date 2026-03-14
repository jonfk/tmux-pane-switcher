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
- A Rust workspace with clear internal boundaries between CLI wiring, core domain logic, tmux integration, process inspection, and SQLite storage
- A long-lived Rust observer manager that maintains one tmux control-mode client per tmux session and observes pane output and metadata changes across the server
- Rust-owned process inspection and heuristic evaluation using tmux metadata plus OS process information
- SQLite, managed by the Rust CLI, as the source of truth for event history and current pane state shared across commands

This is a better fit than a hook-first design because it avoids depending on shell integration, keeps the TPM side minimal, and gives a stronger foundation for state management, process inspection, and ranking logic.

## Core Product Decisions

- Primary tracked unit: pane
- Secondary view: window, derived from the highest-ranked pane in that window
- Default operating mode: zero-integration
- Packaging model:
  - TPM installs only shell wrappers and tmux bindings
  - The Rust CLI is expected to already be installed and available on `PATH`
- Supported signal families:
  - tmux control-mode output notifications such as `%output` and `%extended-output`
  - tmux control-mode lifecycle notifications scoped to each attached session
  - tmux pane metadata such as `pane_current_command`, `pane_pid`, `pane_tty`, `pane_title`, and pane location identifiers
  - semantic terminal signals observed in decoded control-mode output, starting with BEL and `OSC 9`
  - observer-derived activity and silence heuristics computed from decoded control-mode output and timers
  - OS process inspection for the pane's foreground process tree
- Persistence model:
  - append-only event log
  - materialized current pane state table for fast ranking and jumps

## Non-Goals and Constraints

- v1 should not depend on zsh or bash hooks
- v1 should not require programs such as Codex, Claude, test runners, or dev servers to emit custom signals
- v1 should not assume tmux can observe arbitrary macOS Notification Center notifications triggered by programs
- v1 should not rely on tmux `monitor-bell`, `monitor-activity`, or `monitor-silence` semantics
- v1 should not rely on `capture-pane` screen contents alone for semantic escape-sequence detection, because screen capture does not preserve all control bytes observed in control mode
- v1 TPM installation should not be responsible for compiling or installing the Rust core binary
- tmux-observable signals should come from control-mode output and tmux metadata; arbitrary desktop notifications do not appear to be a reliable tmux signal source

Optional explicit program integration may still be added later as a higher-confidence hinting layer, but it should not be required for the base product.

## Distribution Model

Separate distribution concerns between the tmux plugin and the core executable:

- The TPM-managed side should install a shell wrapper, tmux bindings, and minimal configuration glue
- The wrapper should call the Rust CLI and fail clearly if the CLI is not installed
- The Rust CLI should own the observer, SQLite schema and migrations, ranking logic, process inspection, and jump commands
- Internal integration inside the Rust program should happen through in-process modules or crates, not through shell-level command chaining
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

The observer should run as a single long-lived Rust process with an internal session supervisor.

Because a tmux control-mode client can observe only the session to which it is attached, the observer should maintain one control-mode client per tmux session.

The session supervisor should:

- discover sessions on startup with tmux commands such as `list-sessions`
- spawn one control-mode child client per session
- maintain one parser and event stream per child client
- react to session creation and destruction by adding or removing child clients
- periodically reconcile server-wide tmux state so pane identity and liveness remain correct

Each per-session control-mode client should continuously ingest:

- `%output` and `%extended-output` notifications to detect live pane output
- decoded control characters from control-mode output, including BEL (`\007`), to detect bells without relying on tmux alert notifications
- semantic OSC sequences carried in pane output, starting with `OSC 9`
- pane lifecycle changes such as pane death or layout changes
- pane metadata snapshots for panes in its session on startup and during periodic internal consistency checks

The control-mode parser should normalize `%output` and `%extended-output` into the same internal byte-stream event. `%extended-output` carries extra metadata, but the payload should be decoded and parsed the same way as `%output`.

The parser must be streaming, not notification-local:

- tmux octal escapes should be decoded per control-mode notification
- decoded bytes should be appended to a per-pane stream buffer
- an ANSI and OSC state machine should run over the per-pane byte stream
- semantic sequences must be allowed to span multiple `%output` or `%extended-output` notifications

The observer should not assume a single control-mode notification contains a whole OSC sequence. Fragmentation across notifications must be treated as normal.

Control-mode byte observation is more authoritative than screen capture for semantic signals. `capture-pane` does not preserve all OSC or BEL bytes, so semantic signal detection should happen from the decoded control-mode stream rather than from rendered screen contents.

The observer should maintain a live per-pane model with:

- pane identity and location
- pane title and current path where available
- current process classification
- whether the pane is running a shell or a non-shell foreground process
- last output time
- last bell time
- last semantic signal time
- last semantic signal type
- last activity transition time
- last silence transition time
- last inferred "interesting" transition time
- the reason the pane became interesting

The observer should treat live tmux state as authoritative. On startup it should take a full pane snapshot across all sessions, and during runtime it should periodically compare current tmux state against stored state so stale database entries are overwritten or invalidated before they can affect ranking or jumps.

## Validated Control-Mode Observations

Local validation on tmux 3.5a confirmed that control mode exposes raw pane-output bytes for all of the following, in both `%output` and `%extended-output`, as tmux-octal-escaped payloads:

- `OSC 0` with both BEL and ST terminators
- `OSC 7` with both BEL and ST terminators
- `OSC 8` hyperlinks with both BEL and ST terminators
- `OSC 9` with both BEL and ST terminators
- bare BEL (`\007`)

The same validation also confirmed:

- a single OSC sequence may be split across multiple control-mode notifications
- `OSC 0` may both affect tmux state such as `pane_title` and remain visible as raw bytes in control mode
- `capture-pane` does not preserve all of these semantic sequences, so it is insufficient as the primary observation source for them

These findings should be treated as implementation assumptions for tmux 3.5a and captured in fixtures or integration tests.

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
- A pane emits a bell character observed in decoded control-mode output
- A pane emits an explicit semantic signal such as `OSC 9`
- A pane running a non-shell process returns to a shell, suggesting that work has completed
- A pane changes from one classified process type to another in a way that suggests a new task started
- A pane produces new output after a quiet period or produces an output burst that suggests state change

Suggested class-specific behavior:

- `agent`: interesting on output bursts, on idle-after-output, and on process exit back to shell
- `batch`: interesting on start, on failure signals if detectable, on idle-after-output, and on process exit back to shell
- `server_watch`: interesting on bell or unusually meaningful fresh output, but not merely because output stops
- `interactive_other`: interesting while active if the process is not a shell, but generally lower priority than `agent` and `batch`
- `shell`: usually not interesting unless paired with a bell or another heuristic signal

Initial semantic-signal policy:

- bare BEL is a high-confidence interesting signal
- `OSC 9` is a high-confidence interesting signal
- `OSC 0`, `OSC 7`, and `OSC 8` should be parsed and stored as observable semantic output, but they do not need to make a pane interesting in v1 unless later heuristics choose to use them

This avoids the biggest false positive in a generic "silence means done" model: long-lived quiet processes such as servers or SSH sessions that may stop output without having completed.

## Event Model

All observer and internal state-sync paths should normalize their inputs to a common event shape handled by internal Rust services.

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
- `pane_bell`
- `pane_semantic_signal`
- `pane_activity`
- `pane_silence`
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
- `signal_type`
- `signal_terminator`
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
  signal_type TEXT,
  signal_terminator TEXT,
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
  last_bell_at TEXT,
  last_signal_at TEXT,
  last_signal_type TEXT,
  last_activity_at TEXT,
  last_silence_at TEXT,
  last_interest_at TEXT,
  last_process_change_at TEXT,
  rank_updated_at TEXT NOT NULL
);

CREATE INDEX idx_pane_state_rank
ON pane_state(
  server_id,
  is_interesting DESC,
  last_interest_at DESC,
  last_signal_at DESC,
  last_bell_at DESC,
  last_activity_at DESC,
  last_output_at DESC
);
```

## State Update Rules

### On `pane_snapshot`

- Upsert `servers`
- Upsert `panes`
- Refresh pane identity, path, title, pid, command, and liveness in `pane_state`
- If process classification changed, emit `process_classified`
- Mark any previously live panes missing from the latest tmux snapshot as non-running or dead so stale targets stop participating in ranking and jumps

### On `pane_output`

- Insert an `events` row
- Update `pane_state.last_output_at`
- Re-evaluate the pane heuristics

### On `pane_bell`

- Insert an `events` row
- Update `pane_state.last_bell_at`
- Mark the pane interesting with a bell-related `interest_reason`

### On `pane_semantic_signal`

- Insert an `events` row with `signal_type` and `signal_terminator`
- Update `pane_state.last_signal_at` and `pane_state.last_signal_type`
- Re-evaluate the pane heuristics
- If the signal is `osc_9`, mark the pane interesting with a signal-related `interest_reason`

### On `pane_activity`

- Insert an `events` row
- Update `pane_state.last_activity_at`
- Re-evaluate the pane heuristics

### On `pane_silence`

- Insert an `events` row
- Update `pane_state.last_silence_at`
- Re-evaluate the pane heuristics using class-specific idle thresholds

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
- Keep historical events, but ensure the pane is no longer eligible for ranking or jump selection

## Ranking Model

Initial ranking should prefer:

1. Panes that are currently interesting
2. Most recently interesting panes
3. Panes with recent semantic signals such as `OSC 9` or other high-confidence observer-derived transitions
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
  ps.last_signal_at DESC NULLS LAST,
  ps.last_bell_at DESC NULLS LAST,
  ps.last_activity_at DESC NULLS LAST,
  ps.last_output_at DESC NULLS LAST
LIMIT 1;
```

This should support MRU-style switching through panes that have recently become relevant to the user.

Only panes confirmed alive in the latest tmux state should be considered valid jump targets. Historical rows may remain in the database, but stale pane state must not outrank or survive over newer tmux observations.

## Proposed CLI Surface

Keep tmux integration thin by exposing a small Rust CLI surface for top-level operations while keeping event recording, classification, and heuristic evaluation inside internal Rust services.

Commands:

- `observe`
- `list-ranked`
- `jump`
- `doctor`

The TPM wrapper script should call these commands and should not duplicate application logic.

## Proposed Repository Layout

```text
Cargo.toml
Cargo.lock
README.md
tmux-pane-switcher.tmux

scripts/
  pane-switcher-wrapper
  pane-switcher-observer

crates/
  tps-cli/
    src/
      main.rs
      commands/
        observe.rs
        list_ranked.rs
        jump.rs
        doctor.rs
  tps-core/
    src/
      event.rs
      pane.rs
      pane_state.rs
      process_class.rs
      classifier.rs
      heuristics.rs
      ranking.rs
      jump.rs
      reconcile.rs
      ports.rs
  tps-tmux/
    src/
      control_client.rs
      control_parser.rs
      ansi_parser.rs
      snapshot.rs
      commands.rs
      ids.rs
  tps-store/
    src/
      db.rs
      migrations.rs
      event_repo.rs
      pane_repo.rs
      ranking_repo.rs
  tps-process/
    src/
      inspector.rs
      macos.rs
      process_tree.rs

sql/
  0001_init.sql
  0002_indexes.sql

docs/
  architecture.md
  installation.md
  configuration.md
  heuristics.md
  process-classification.md
  observer-lifecycle.md

tests/
  integration/
    observe_flow.rs
    ranking_flow.rs
    jump_flow.rs
  fixtures/
    control_mode/
    process_trees/
    sqlite/
```

## Implementation Phases

### Phase 1: Plugin Skeleton and Core Persistence

- Add TPM-compatible `tmux-pane-switcher.tmux`
- Add a shell wrapper that locates and invokes the Rust CLI
- Add a Rust workspace with initial crates for CLI wiring, core logic, tmux integration, storage, and process inspection
- Add SQLite schema and initialization path owned by the Rust CLI, with automatic startup initialization for commands that require the database
- Add a Phase 1 `observe` command that performs one startup snapshot and exits; it should not yet run as a long-lived control-mode observer
- Add Rust CLI commands for:
  - one-shot startup `observe`
  - ranking query
  - jump
- Add minimal tmux command bindings

Acceptance criteria:

- The wrapper can invoke the installed Rust CLI
- The Rust CLI initializes its database
- The Phase 1 `observe` command performs a startup pane snapshot, updates pane state, and exits
- The plugin can query the top-ranked pane target

### Phase 2: Control-Mode Observer

- Extend `observe` from the Phase 1 one-shot startup snapshot into a long-lived Rust observer manager process that maintains one tmux control-mode client per session
- Ingest `%output`, `%extended-output`, and related pane notifications from every observed session
- Perform a full pane snapshot on startup and periodic internal consistency checks during runtime
- Decode control-mode octal escapes into per-pane byte streams
- Run a streaming ANSI and OSC parser over each per-pane stream so fragmented sequences are handled correctly
- Detect BEL directly from the decoded byte stream
- Detect `OSC 9` directly from the decoded byte stream, with support for both BEL and ST terminators
- Persist output activity and pane lifecycle changes

Acceptance criteria:

- Pane output updates `last_output_at`
- Bell characters observed in output produce `pane_bell` events
- `OSC 9` observed in either `%output` or `%extended-output` produces `pane_semantic_signal` events
- A fragmented `OSC 9` split across multiple control-mode notifications is parsed correctly
- New and dead panes are reflected in state
- New sessions cause a new control-mode client to be started automatically
- Observer restarts restore accurate current pane state without corrupting history

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
- Re-implement bell, activity, and silence as observer-driven heuristics instead of tmux alert semantics
- Treat `OSC 9` as a high-confidence interesting signal in ranking and state transitions
- Rank panes by `is_interesting`, `last_interest_at`, heuristic transition recency, and output recency

Acceptance criteria:

- Agent and batch panes become interesting after likely completion or meaningful state changes
- Server and watch panes do not become false positives just because output stops
- Bell and output-driven heuristic transitions are recorded without enabling tmux monitor alerts
- `OSC 9` transitions are recorded and ranked as interesting
- Ranked queries match the intended MRU behavior

### Phase 5: Jump and Selection UX

- Add `jump`
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
- The docs clearly explain that control-mode observation is session-scoped, so the observer maintains one control-mode client per session
- The docs clearly explain the difference between observer-derived signals and arbitrary OS notifications
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

- exact tmux control-mode commands, per-session child-client lifecycle, and supervisor reconciliation behavior
- exact normalization rules for `%output` and `%extended-output`, including octal decoding and per-pane stream buffering
- exact ANSI and OSC parser state-machine behavior, including fragmented sequence handling and BEL versus ST termination
- exact Rust CLI command names and wrapper invocation contract
- exact SQLite initialization path
- exact internal service boundaries between CLI wiring, event normalization, heuristics, and persistence
- the initial classification table and heuristic rules
- acceptance tests for pane snapshots, output ingestion, ranking, and pane jumps
