# tmux-pane-switcher
- A tmux plugin
- configuration should be enabled through the tmux.conf
- A tmux pane/window switcher that tracks notifications and pending programs running from each pane or window.
- It would order the panes by most recently notified or started/pending program so that the user can switch quickly to a recent pane.
- The idea is to make easy coming back to a pane where the user started an async thing across sessions. This would also give visibility to these async actions.
    - Some examples of async actions are: 
        - Start a dev server
        - Start a compilation run or test run
        - An agent turn in a coding agent (codex/claude)
