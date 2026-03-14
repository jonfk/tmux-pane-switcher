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
            control_bytes = b"".join(
                event.payload
                for line in notifications
                if (event := parse_output_notification(line)) is not None
            )

        capture = server.capture_pane(alpha_pane)

        assert_true(b"\x1b]0;osc0-bell-title\x07" in control_bytes, "control stream did not contain OSC 0")
        assert_true(b"\x1b]9;osc9-bell-message\x07" in control_bytes, "control stream did not contain OSC 9")
        assert_true(b"\x07" in control_bytes, "control stream did not contain BEL")
        assert_true("\x1b]" not in capture, "capture-pane unexpectedly preserved OSC bytes")
        assert_true("\x07" not in capture, "capture-pane unexpectedly preserved BEL bytes")

        print("PASS validate_capture_vs_control")
        print(f"pane_id={alpha_pane}")
        print(f"control_byte_count={len(control_bytes)}")
        print(f"capture_char_count={len(capture)}")
        print("capture_excerpt=")
        for line in capture.splitlines()[-8:]:
            print(f"  {line}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

