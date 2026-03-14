#!/usr/bin/env python3

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from tmux_test_utils import (  # noqa: E402
    ControlModeClient,
    TempTmuxServer,
    assert_true,
)


def main() -> int:
    with TempTmuxServer() as server:
        server.new_session("alpha", command=server.shell())
        alpha_window = server.display("#{window_id}", target="alpha:0.0")
        initial_panes = set(server.list_panes("#{pane_id}"))

        with ControlModeClient(server, "alpha") as client:
            created_pane = server.split_window("alpha:0.0", command=server.shell())
            time.sleep(0.25)
            server.cmd("kill-pane", "-t", created_pane)
            time.sleep(0.25)

            server.new_session("beta", command=server.shell())
            time.sleep(0.25)
            server.cmd("kill-session", "-t", "beta")
            time.sleep(0.25)

            server.cmd("set-window-option", "-t", alpha_window, "remain-on-exit", "on")
            dead_pane = server.split_window("alpha:0.0", command="printf 'dead-pane-test\\n'")
            server.wait_for(
                lambda: server.display("#{pane_dead}", target=dead_pane) == "1",
                timeout=2.0,
                description="dead pane with remain-on-exit",
            )

            notifications = client.notifications(client.drain_lines(timeout=2.0))

            assert_true(
                any(line.startswith(f"%layout-change {alpha_window} ") for line in notifications),
                "expected %layout-change during pane split or kill",
            )
            assert_true(
                notifications.count("%sessions-changed") >= 2,
                "expected %sessions-changed for session create and destroy",
            )
            assert_true(
                not any(line.startswith("%pane-exited") or line.startswith("%pane-died") for line in notifications),
                "unexpected pane exit notification appeared in control mode",
            )

            current_panes = set(server.list_panes("#{pane_id}"))
            dead_state = server.display(
                "#{pane_dead}|#{pane_dead_status}|#{pane_dead_signal}|#{pane_current_command}",
                target=dead_pane,
            )

            assert_true(created_pane not in current_panes, "killed pane still present after kill-pane")
            assert_true(dead_pane in current_panes, "remain-on-exit pane missing from snapshot")
            assert_true(dead_state.startswith("1|"), "remain-on-exit pane did not expose pane_dead metadata")

            print("PASS validate_lifecycle_events")
            print(f"window_id={alpha_window}")
            print(f"initial_pane_count={len(initial_panes)}")
            print(f"current_pane_count={len(current_panes)}")
            print(f"dead_pane_state={dead_state}")
            print("observed_notifications=")
            for line in notifications:
                print(f"  {line}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

