#!/usr/bin/env bash
set -euo pipefail

current_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

observe_key="$(tmux show-option -gqv '@pane-switcher-observe-key' || true)"
jump_key="$(tmux show-option -gqv '@pane-switcher-jump-key' || true)"

if [[ -z "${observe_key}" ]]; then
  observe_key="O"
fi

if [[ -z "${jump_key}" ]]; then
  jump_key="o"
fi

tmux bind-key "${observe_key}" run-shell "${current_dir}/scripts/pane-switcher-wrapper observe"
tmux bind-key "${jump_key}" run-shell "${current_dir}/scripts/pane-switcher-wrapper jump"
