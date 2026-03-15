#!/usr/bin/env python3

from __future__ import annotations

import shlex
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


def collect_until_output(
    client: ControlModeClient,
    pane_id: str,
    predicate,
    timeout: float,
) -> tuple[list[str], list]:
    deadline = time.monotonic() + timeout
    notifications: list[str] = []
    outputs = []

    while time.monotonic() < deadline:
        remaining = max(deadline - time.monotonic(), 0.0)
        batch = client.notifications(
            client.drain_lines(timeout=min(0.5, remaining), quiet_period=0.05)
        )
        for line in batch:
            notifications.append(line)
            event = parse_output_notification(line)
            if event is not None:
                outputs.append(event)
            if line == f"%pause {pane_id}":
                client.command_output(f"refresh-client -A {pane_id}:continue")
        if predicate(outputs):
            return notifications, outputs

    return notifications, outputs


def shell_command(script: str) -> str:
    return f"sh -lc {shlex.quote(script)}"


def main() -> int:
    plain_marker = "plain-output-marker-001"
    extended_marker = "extended-output-marker-001"

    with TempTmuxServer() as server:
        server.new_session("alpha", command="sleep 30")

        with ControlModeClient(server, "alpha") as client:
            plain_pane = server.split_window(
                "alpha:0.0",
                command=shell_command(f"printf '{plain_marker}\\n'; sleep 0.2"),
            )
            first_batch, first_outputs = collect_until_output(
                client,
                plain_pane,
                lambda events: any(
                    event.kind == "%output"
                    and event.pane_id == plain_pane
                    and plain_marker.encode() in event.payload
                    for event in events
                ),
                timeout=2.0,
            )

            assert_true(
                any(event.kind == "%output" and plain_marker.encode() in event.payload for event in first_outputs),
                "expected %output notification before pause-after was enabled",
            )

            client.command_output("refresh-client -f pause-after=0.050")
            client.notifications(client.drain_lines(timeout=0.3, quiet_period=0.05))

            extended_pane = server.split_window(
                "alpha:0.0",
                command=shell_command(
                    f"printf '{extended_marker}\\n'; yes '{extended_marker}-burst' | head -n 200"
                ),
            )
            second_batch, second_outputs = collect_until_output(
                client,
                extended_pane,
                lambda events: any(
                    event.kind == "%extended-output"
                    and event.pane_id == extended_pane
                    and extended_marker.encode() in event.payload
                    for event in events
                ),
                timeout=4.0,
            )

            extended_events = [
                event
                for event in second_outputs
                if event.kind == "%extended-output" and event.pane_id == extended_pane
            ]

            assert_true(
                any(extended_marker.encode() in event.payload for event in extended_events),
                "expected %extended-output notification after pause-after was enabled",
            )
            assert_true(
                all(event.age_ms is not None for event in extended_events),
                "expected age metadata on %extended-output notifications",
            )

            print("PASS validate_output_notifications")
            print(f"plain_pane_id={plain_pane}")
            print(f"extended_pane_id={extended_pane}")
            print(
                f"plain_output_events={sum(event.kind == '%output' and event.pane_id == plain_pane for event in first_outputs)}"
            )
            print(f"extended_output_events={len(extended_events)}")
            print("observed_notifications=")
            for line in second_batch:
                print(f"  {line}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
