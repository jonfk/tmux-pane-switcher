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
    plain_marker = "plain-output-marker-001"
    extended_marker = "extended-output-marker-001"

    with TempTmuxServer() as server:
        server.new_session("alpha", command=server.shell())
        alpha_pane = server.display("#{pane_id}", target="alpha:0.0")

        with ControlModeClient(server, "alpha") as client:
            server.send_shell_line(alpha_pane, f"printf '{plain_marker}\\n'")
            time.sleep(0.3)
            first_batch = client.notifications(client.drain_lines(timeout=1.0))
            first_outputs = [
                event
                for line in first_batch
                if (event := parse_output_notification(line)) is not None
            ]

            assert_true(
                any(event.kind == "%output" and plain_marker.encode() in event.payload for event in first_outputs),
                "expected %output notification before pause-after was enabled",
            )

            client.command("refresh-client -f pause-after=0.050")
            time.sleep(0.1)
            client.drain_lines(timeout=0.3)

            server.send_shell_line(
                alpha_pane,
                f"printf '{extended_marker}\\n'; yes '{extended_marker}-burst' | head -n 200",
            )
            time.sleep(0.5)
            second_batch = client.notifications(client.drain_lines(timeout=1.5))
            second_outputs = [
                event
                for line in second_batch
                if (event := parse_output_notification(line)) is not None
            ]

            extended_events = [event for event in second_outputs if event.kind == "%extended-output"]

            assert_true(
                any(extended_marker.encode() in event.payload for event in extended_events),
                "expected %extended-output notification after pause-after was enabled",
            )
            assert_true(
                all(event.age_ms is not None for event in extended_events),
                "expected age metadata on %extended-output notifications",
            )

            print("PASS validate_output_notifications")
            print(f"pane_id={alpha_pane}")
            print(f"plain_output_events={sum(event.kind == '%output' for event in first_outputs)}")
            print(f"extended_output_events={len(extended_events)}")
            print("observed_notifications=")
            for line in second_batch:
                print(f"  {line}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

