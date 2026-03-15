# tmux-pane-switcher

- A tmux plugin
- Configuration should be enabled through `tmux.conf`
- A tmux pane/window switcher that observes panes and ranks the ones that are currently or recently "interesting"
- It should let the user jump back to panes in most recently used order when those panes are doing useful work or have likely changed state
- The goal is to make it easy to come back to a pane where the user started an async or semi-async thing across sessions, while also giving visibility into that work
- The core implementation should live in a separately installed Rust CLI, while the TPM-managed plugin should stay a thin shell wrapper that calls that CLI

Examples of panes that may become interesting:

- A coding agent turn in a coding agent such as Codex or Claude
- A compilation run or test run that has produced output and then gone quiet
- A program that rings a bell or triggers a tmux alert
- A pane running a non-shell foreground process that is likely the reason the user will want to return

The key idea is to use heuristics instead of relying primarily on tmux hooks or shell integration:

- Observe pane metadata and output from tmux itself
- Classify the foreground program or process tree in each pane
- Use heuristics to decide when a pane becomes interesting
- Rank interesting panes by recency so the user can switch through them quickly

Architecture direction:

- TPM should install only the tmux-facing wrapper script and config glue
- The Rust CLI should own observation, state management, ranking, and jump target resolution, while the tmux wrapper performs the client jump
- Installing or upgrading the Rust program should be managed separately from TPM

Important limitation:

- tmux can observe pane output, bells, and tmux alerts
- tmux cannot be assumed to expose arbitrary macOS Notification Center notifications requested by programs
- v1 should therefore focus on tmux-observable signals and process heuristics, not direct OS notification capture
