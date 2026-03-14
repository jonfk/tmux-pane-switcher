# Experiments

These scripts validate tmux behaviors that the project plan depends on.

They are designed to be rerun in the future against a fresh tmux version or a different local setup. Each validator starts its own temporary tmux server, so it should not interfere with an existing tmux session.

## Shared Helpers

### `tmux_test_utils.py`

Shared helper module used by the validators.

It provides:

- temporary isolated tmux server setup
- a small control-mode client wrapper
- parsing for `%output` and `%extended-output`
- tmux-octal decoding for control-mode payloads
- polling and assertion helpers for repeatable checks

### `osc_emitter.py`

Helper program that emits plain text, BEL, and OSC sequences.

It is used to validate what tmux exposes through control mode and what is lost in `capture-pane`.

## Validators

### `validate_control_scope.py`

Validates control-mode session scoping.

It checks that:

- a control-mode client attached to session `alpha` receives pane output from `alpha`
- the same client does not receive pane output from another session
- server-wide notifications such as `%sessions-changed` are still visible

This script is important for validating the plan’s assumption that one control-mode client is needed per tmux session for pane-output observation.

### `validate_output_notifications.py`

Validates `%output`, `%extended-output`, and `pause-after`.

It checks that:

- normal pane output arrives as `%output`
- enabling `pause-after` changes output delivery to `%extended-output`
- `%extended-output` carries age metadata
- buffered output can lead to `%pause`

This script is important for validating the event-ingestion path and the parser assumptions around `%extended-output`.

### `validate_semantic_sequences.py`

Validates semantic control bytes in control-mode output.

It checks that control mode exposes:

- bare BEL
- `OSC 0`
- `OSC 7`
- `OSC 8`
- `OSC 9`
- both BEL and ST terminators
- fragmented `OSC 9` sequences split across writes

It also records the resulting `pane_title` after the `OSC 0` test so the interaction between raw control-mode bytes and tmux pane metadata can be rechecked.

### `validate_capture_vs_control.py`

Validates the difference between control-mode output and `capture-pane`.

It checks that:

- semantic bytes such as OSC sequences and BEL appear in control-mode output
- those same bytes are not preserved by `capture-pane`

This script is important for validating the design decision to detect semantic signals from the decoded control-mode byte stream instead of relying on rendered screen capture.

### `validate_jump_identity.py`

Validates pane identity and jump targeting.

It checks that:

- `session_id`, `window_id`, and `pane_id` remain stable across session rename, window rename, and window move within the same tmux server
- a client can jump to the target pane using the sequence:
  - `switch-client`
  - `select-window`
  - `select-pane`

This script is important for validating the jump model in the plan.

### `validate_lifecycle_events.py`

Validates lifecycle-related notifications and snapshot metadata.

It checks that:

- pane splits and pane removal cause `%layout-change`
- session creation and destruction cause `%sessions-changed`
- pane exit is not surfaced as a direct control-mode notification in this setup
- `remain-on-exit` panes stay visible in snapshots and expose dead-pane metadata such as `pane_dead`

This script is important for validating which lifecycle transitions can be observed directly from control mode and which must be inferred from periodic reconciliation or snapshot deltas.

### `validate_process_metadata.py`

Validates tmux pane process metadata.

It checks that:

- `pane_current_path` updates after `cd`
- `pane_current_command` changes when a foreground child process runs
- `pane_current_command` returns to the shell afterward
- `pane_pid` remains the pane’s base process PID through the child-process transition

This script is important for validating which fields can be populated directly from tmux and where tmux metadata alone is insufficient for richer foreground-process classification.

### `validate_snapshot_parsing.py`

Validates snapshot field parsing assumptions.

It checks that:

- tmux escapes tabs in `window_name` when using `list-panes -F`
- tmux leaves newlines in `pane_current_path` raw, which breaks delimiter-based row parsing
- the `tps-tmux` fake-tmux regression test for tabs/newlines passes

This script is important for validating both the tmux-side behavior behind the bug report and the crate-side regression test that proves the parser fix.

## Notes

- These validators currently reflect observed behavior on tmux `3.5a`.
- They are behavioral checks, not full integration tests for the future Rust implementation.
- If a validator starts failing after a tmux upgrade, the failure may indicate either a regression in the experiment or a real tmux behavior change that should be reflected in `PLAN.md`.

## Snapshot Parsing Experiment

Run it from the repo root:

```sh
python3 experiments/validate_snapshot_parsing.py
```

Expected result:

- the script prints `PASS validate_snapshot_parsing`
- the `window_name_bytes` line contains `5c 74` for `\t`
- the `pane_current_path_bytes` line contains a literal `0a` newline byte inside the path payload
- the cargo test tail shows `collect_snapshot_handles_tabs_and_newlines ... ok`
