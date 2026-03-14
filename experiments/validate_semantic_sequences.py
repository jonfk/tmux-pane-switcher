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


def main() -> int:
    root = Path(__file__).resolve().parent
    emitter = root / "osc_emitter.py"

    with TempTmuxServer() as server:
        server.new_session("alpha", command=server.shell())
        alpha_pane = server.display("#{pane_id}", target="alpha:0.0")

        with ControlModeClient(server, "alpha") as client:
            server.send_shell_line(alpha_pane, f"python3 {shlex.quote(str(emitter))} --pause 0.02")
            time.sleep(1.5)
            notifications = client.notifications(client.drain_lines(timeout=2.0))
            output_bytes = b"".join(
                event.payload
                for line in notifications
                if (event := parse_output_notification(line)) is not None
            )

            required_sequences = {
                "osc0-bel": b"\x1b]0;osc0-bell-title\x07",
                "osc0-st": b"\x1b]0;osc0-st-title\x1b\\",
                "osc7-bel": b"\x1b]7;file:///tmp/osc-seven\x07",
                "osc7-st": b"\x1b]7;file:///tmp/osc-seven-st\x1b\\",
                "osc8-bel": b"\x1b]8;;https://example.com/osc8\x07osc8-link\x1b]8;;\x07",
                "osc8-st": b"\x1b]8;;https://example.com/osc8-st\x1b\\osc8-link-st\x1b]8;;\x1b\\",
                "osc9-bel": b"\x1b]9;osc9-bell-message\x07",
                "osc9-st": b"\x1b]9;osc9-st-message\x1b\\",
                "osc9-fragmented-bel": b"\x1b]9;osc9-fragmented-message\x07",
                "osc9-fragmented-st": b"\x1b]9;osc9-fragmented-st-message\x1b\\",
                "bel-byte": b"\x07",
            }

            missing = [
                label
                for label, sequence in required_sequences.items()
                if sequence not in output_bytes
            ]
            assert_true(
                not missing,
                f"missing semantic sequences from control output: {', '.join(missing)}",
            )

            pane_title = server.display("#{pane_title}", target=alpha_pane)

            print("PASS validate_semantic_sequences")
            print(f"pane_id={alpha_pane}")
            print(f"pane_title_after_run={pane_title}")
            print(f"captured_bytes={len(output_bytes)}")
            print("validated_sequences=")
            for label in required_sequences:
                print(f"  {label}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

