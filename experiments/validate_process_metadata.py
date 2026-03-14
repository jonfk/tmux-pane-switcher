#!/usr/bin/env python3

from __future__ import annotations

import shlex
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from tmux_test_utils import (  # noqa: E402
    TempTmuxServer,
    assert_true,
)


def main() -> int:
    with TempTmuxServer() as server:
        working_dir = server.root / "cwd-check"
        working_dir.mkdir(parents=True, exist_ok=True)

        server.new_session("alpha")
        alpha_pane = server.display("#{pane_id}", target="alpha:0.0")

        initial = server.display(
            "#{pane_pid}|#{pane_current_command}|#{pane_current_path}",
            target=alpha_pane,
        ).split("|")
        initial_pid, initial_command, _ = initial

        server.send_shell_line(alpha_pane, f"cd {shlex.quote(str(working_dir))}")
        def cwd_matches() -> bool:
            current = server.display("#{pane_current_path}", target=alpha_pane)
            if not current:
                return False
            return Path(current).resolve() == working_dir.resolve()

        server.wait_for(cwd_matches, timeout=3.0, description="pane_current_path update after cd")

        server.send_shell_line(alpha_pane, "python3 -c 'import time; time.sleep(1.0)'")
        server.wait_for(
            lambda: "python" in server.display("#{pane_current_command}", target=alpha_pane).lower(),
            timeout=2.0,
            description="pane_current_command change to a python process",
        )
        during = server.display(
            "#{pane_pid}|#{pane_current_command}|#{pane_current_path}",
            target=alpha_pane,
        ).split("|")

        server.wait_for(
            lambda: server.display("#{pane_current_command}", target=alpha_pane) == initial_command,
            timeout=3.0,
            description="pane_current_command return to shell",
        )
        after = server.display(
            "#{pane_pid}|#{pane_current_command}|#{pane_current_path}",
            target=alpha_pane,
        ).split("|")

        assert_true(during[0] == initial_pid, "pane_pid changed while foreground process changed")
        assert_true(
            "python" in during[1].lower(),
            "pane_current_command did not reflect a foreground python child process",
        )
        assert_true(after[0] == initial_pid, "pane_pid changed after child process exit")
        assert_true(after[1] == initial_command, "pane_current_command did not return to shell")
        assert_true(Path(after[2]).resolve() == working_dir.resolve(), "pane_current_path did not persist after cd")

        print("PASS validate_process_metadata")
        print(f"pane_id={alpha_pane}")
        print(f"initial_pid={initial_pid}")
        print(f"initial_command={initial_command}")
        print(f"during_child={during}")
        print(f"after_child={after}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
