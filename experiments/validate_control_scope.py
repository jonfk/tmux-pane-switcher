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
    parse_output_notification,
)


def main() -> int:
    alpha_marker = "alpha-scope-marker-001"
    beta_marker = "beta-scope-marker-001"

    with TempTmuxServer() as server:
        server.new_session("alpha", command=server.shell())
        server.new_session("beta", command=server.shell())

        alpha_pane = server.display("#{pane_id}", target="alpha:0.0")
        beta_pane = server.display("#{pane_id}", target="beta:0.0")

        with ControlModeClient(server, "alpha") as client:
            server.send_shell_line(beta_pane, f"printf '{beta_marker}\\n'")
            time.sleep(0.25)
            server.send_shell_line(alpha_pane, f"printf '{alpha_marker}\\n'")
            time.sleep(0.25)
            server.new_session("gamma", command=server.shell())
            time.sleep(0.35)

            notifications = client.notifications(client.drain_lines(timeout=1.5))
            outputs = [
                event
                for line in notifications
                if (event := parse_output_notification(line)) is not None
            ]

            alpha_seen = any(
                event.pane_id == alpha_pane and alpha_marker.encode() in event.payload
                for event in outputs
            )
            beta_seen = any(
                event.pane_id == beta_pane and beta_marker.encode() in event.payload
                for event in outputs
            )

            assert_true(alpha_seen, "attached-session control client did not receive alpha pane output")
            assert_true(not beta_seen, "attached-session control client unexpectedly received beta pane output")
            assert_true(
                any(line == "%sessions-changed" for line in notifications),
                "control client did not receive %sessions-changed for session creation",
            )

            print("PASS validate_control_scope")
            print(f"attached_session=alpha attached_pane={alpha_pane}")
            print(f"other_session=beta other_pane={beta_pane}")
            print("observed_notifications=")
            for line in notifications:
                print(f"  {line}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

