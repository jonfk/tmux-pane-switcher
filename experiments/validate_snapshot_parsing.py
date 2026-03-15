#!/usr/bin/env python3

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from tmux_test_utils import (  # noqa: E402
    TempTmuxServer,
    assert_true,
    fail,
)


def hex_bytes(value: bytes) -> str:
    return " ".join(f"{byte:02x}" for byte in value)


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent

    with TempTmuxServer() as server:
        newline_dir = server.root / "with\nline"
        newline_dir.mkdir(parents=True, exist_ok=True)

        server.new_session(
            "alpha",
            command=server.shell(),
            start_directory=str(newline_dir),
        )
        pane_id = server.display("#{pane_id}", target="alpha:0.0")

        server.cmd("rename-window", "-t", "alpha:0", "win\tname")
        server.cmd("select-pane", "-t", pane_id, "-T", "pane title\nsecond line")

        window_name_bytes = server.cmd(
            "list-panes",
            "-a",
            "-F",
            "#{window_name}",
            text=False,
        ).stdout
        current_path_bytes = server.cmd(
            "list-panes",
            "-a",
            "-F",
            "#{pane_current_path}",
            text=False,
        ).stdout

        assert_true(
            b"win\\tname\n" in window_name_bytes,
            "tmux did not escape the tab in window_name as backslash-t",
        )
        assert_true(
            b"win\tname\n" not in window_name_bytes,
            "tmux exposed a raw tab in window_name output",
        )
        assert_true(
            b"with\nline\n" in current_path_bytes,
            "tmux did not expose the pane_current_path newline as a raw byte",
        )
        assert_true(
            b"with\\nline\n" not in current_path_bytes,
            "tmux escaped the pane_current_path newline unexpectedly",
        )

    cargo_test = subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "tps-tmux",
            "collect_snapshot_handles_tabs_and_newlines",
            "--",
            "--nocapture",
        ],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if cargo_test.returncode != 0:
        detail = cargo_test.stderr.strip() or cargo_test.stdout.strip() or str(cargo_test.returncode)
        fail(f"cargo test failed: {detail}")

    print("PASS validate_snapshot_parsing")
    print(f"pane_id={pane_id}")
    print(f"window_name_bytes={hex_bytes(window_name_bytes)}")
    print(f"pane_current_path_bytes={hex_bytes(current_path_bytes)}")
    print("cargo_test=collect_snapshot_handles_tabs_and_newlines")
    print("cargo_test_tail=")
    for line in cargo_test.stdout.strip().splitlines()[-4:]:
        print(f"  {line}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
