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


def pane_row(server: TempTmuxServer, pane_id: str) -> list[str]:
    for row in server.list_panes("#{session_id}|#{session_name}|#{window_id}|#{window_index}|#{pane_id}"):
        parts = row.split("|")
        if len(parts) == 5 and parts[4] == pane_id:
            return parts
    raise RuntimeError(f"pane row not found for {pane_id}")


def current_client_row(server: TempTmuxServer) -> str:
    rows = server.cmd(
        "list-clients",
        "-F",
        "#{session_id}|#{window_id}|#{pane_id}",
    ).stdout.splitlines()
    if not rows:
        raise RuntimeError("no tmux clients are attached")
    return rows[0]


def main() -> int:
    with TempTmuxServer() as server:
        server.new_session("alpha", command=server.shell())
        server.new_session("beta", command=server.shell())
        server.new_window("alpha", window_name="extra", command=server.shell())
        target_pane = server.split_window("alpha:extra", command=server.shell())

        original = pane_row(server, target_pane)
        session_id, _, window_id, _, pane_id = original

        server.cmd("rename-session", "-t", "alpha", "alpha-renamed")
        server.cmd("rename-window", "-t", window_id, "renamed-window")
        server.cmd("move-window", "-s", window_id, "-t", "alpha-renamed:7")

        renamed = pane_row(server, target_pane)

        assert_true(renamed[0] == session_id, "session_id changed after rename")
        assert_true(renamed[2] == window_id, "window_id changed after rename or move")
        assert_true(renamed[4] == pane_id, "pane_id changed after window move")

        with ControlModeClient(server, "beta") as client:
            client.command(f"switch-client -t {session_id}")
            time.sleep(0.1)
            client.command(f"select-window -t {window_id}")
            time.sleep(0.1)
            client.command(f"select-pane -t {pane_id}")
            server.wait_for(
                lambda: current_client_row(server) == f"{session_id}|{window_id}|{pane_id}",
                timeout=2.0,
                description="client to switch to the target pane",
            )
            current = current_client_row(server)
            assert_true(
                current == f"{session_id}|{window_id}|{pane_id}",
                "client did not land on the expected pane after id-based jump sequence",
            )

            print("PASS validate_jump_identity")
            print(f"session_id={session_id}")
            print(f"window_id={window_id}")
            print(f"pane_id={pane_id}")
            print(f"session_name_after_rename={renamed[1]}")
            print(f"window_index_after_move={renamed[3]}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
