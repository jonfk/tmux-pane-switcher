#!/usr/bin/env python3

from __future__ import annotations

import os
import queue
import subprocess
import tempfile
import threading
import time
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Callable


TMUX_BIN = os.environ.get("TMUX_BIN", "tmux")


class ExperimentFailure(RuntimeError):
    pass


def fail(message: str) -> "NoReturn":
    raise ExperimentFailure(message)


def assert_true(condition: bool, message: str) -> None:
    if not condition:
        fail(message)


def decode_tmux_escaped(value: str) -> bytes:
    decoded = bytearray()
    index = 0

    while index < len(value):
        char = value[index]
        if char != "\\":
            decoded.extend(char.encode("utf-8"))
            index += 1
            continue

        octal = value[index + 1 : index + 4]
        if len(octal) == 3 and all(digit in "01234567" for digit in octal):
            decoded.append(int(octal, 8))
            index += 4
            continue

        if index + 1 < len(value):
            decoded.extend(value[index + 1].encode("utf-8"))
            index += 2
            continue

        decoded.extend(char.encode("utf-8"))
        index += 1

    return bytes(decoded)


@dataclass
class OutputNotification:
    kind: str
    pane_id: str
    payload: bytes
    raw_line: str
    age_ms: int | None = None


def parse_output_notification(line: str) -> OutputNotification | None:
    if line.startswith("%output "):
        _, pane_id, encoded = line.split(" ", 2)
        return OutputNotification(
            kind="%output",
            pane_id=pane_id,
            payload=decode_tmux_escaped(encoded),
            raw_line=line,
        )

    if line.startswith("%extended-output "):
        prefix, separator, encoded = line.partition(" : ")
        if not separator:
            return None
        parts = prefix.split()
        if len(parts) < 3:
            return None
        return OutputNotification(
            kind="%extended-output",
            pane_id=parts[1],
            age_ms=int(parts[2]),
            payload=decode_tmux_escaped(encoded),
            raw_line=line,
        )

    return None


class TempTmuxServer:
    def __init__(self) -> None:
        self._tmpdir: tempfile.TemporaryDirectory[str] | None = None
        self.root: Path | None = None
        self.socket_path: Path | None = None
        self.config_path: Path | None = None

    def __enter__(self) -> "TempTmuxServer":
        self._tmpdir = tempfile.TemporaryDirectory(prefix="tmux-pane-switcher-")
        self.root = Path(self._tmpdir.name)
        self.socket_path = self.root / "tmux.sock"
        self.config_path = self.root / "tmux.conf"
        self.config_path.write_text("", encoding="utf-8")
        self.cmd("start-server")
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        try:
            self.cmd("kill-server", check=False)
        finally:
            if self._tmpdir is not None:
                self._tmpdir.cleanup()

    def base_cmd(self) -> list[str]:
        assert self.socket_path is not None
        assert self.config_path is not None
        return [TMUX_BIN, "-f", str(self.config_path), "-S", str(self.socket_path)]

    def cmd(
        self,
        *args: str,
        check: bool = True,
        capture_output: bool = True,
        text: bool = True,
    ) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            [*self.base_cmd(), *args],
            check=False,
            capture_output=capture_output,
            text=text,
        )
        if check and result.returncode != 0:
            stderr = result.stderr.strip() if result.stderr else ""
            stdout = result.stdout.strip() if result.stdout else ""
            detail = stderr or stdout or f"exit code {result.returncode}"
            fail(f"tmux command failed: {' '.join(args)}: {detail}")
        return result

    def new_session(
        self,
        name: str,
        command: str | None = None,
        start_directory: str | None = None,
    ) -> None:
        args = ["new-session", "-d", "-s", name, "-x", "120", "-y", "40"]
        if start_directory is not None:
            args.extend(["-c", start_directory])
        if command is not None:
            args.append(command)
        self.cmd(*args)

    def new_window(
        self,
        session: str,
        window_name: str | None = None,
        command: str | None = None,
    ) -> None:
        args = ["new-window", "-d", "-t", session]
        if window_name is not None:
            args.extend(["-n", window_name])
        if command is not None:
            args.append(command)
        self.cmd(*args)

    def split_window(
        self,
        target: str,
        command: str | None = None,
        detached: bool = True,
    ) -> str:
        args = ["split-window"]
        if detached:
            args.append("-d")
        args.extend(["-P", "-F", "#{pane_id}", "-t", target])
        if command is not None:
            args.append(command)
        return self.cmd(*args).stdout.strip()

    def shell(self) -> str:
        return os.environ.get("SHELL", "/bin/sh")

    def display(self, fmt: str, target: str | None = None) -> str:
        args = ["display-message", "-p"]
        if target is not None:
            args.extend(["-t", target])
        args.append(fmt)
        return self.cmd(*args).stdout.strip()

    def send_shell_line(self, target: str, line: str) -> None:
        self.cmd("send-keys", "-t", target, line, "C-m")

    def capture_pane(self, target: str, start_line: int = -100) -> str:
        return self.cmd(
            "capture-pane",
            "-p",
            "-S",
            str(start_line),
            "-t",
            target,
        ).stdout

    def list_panes(self, fmt: str, all_panes: bool = True) -> list[str]:
        args = ["list-panes"]
        if all_panes:
            args.append("-a")
        args.extend(["-F", fmt])
        output = self.cmd(*args).stdout
        return [line for line in output.splitlines() if line]

    def wait_for(
        self,
        predicate: Callable[[], bool],
        timeout: float,
        description: str,
        interval: float = 0.05,
    ) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(interval)
        fail(f"timed out waiting for {description}")


class ControlModeClient:
    def __init__(
        self,
        server: TempTmuxServer,
        session: str,
        client_flags: list[str] | None = None,
    ) -> None:
        self.server = server
        self._queue: queue.Queue[str | None] = queue.Queue()
        self._buffer: deque[str] = deque()
        self._process = self._spawn(session, client_flags or [])
        self._reader = threading.Thread(target=self._read_stdout, daemon=True)
        self._reader.start()
        time.sleep(0.15)
        self.drain_lines(timeout=0.3)

    def _spawn(self, session: str, client_flags: list[str]) -> subprocess.Popen[str]:
        command = [*self.server.base_cmd(), "-C", "attach-session"]
        if client_flags:
            command.extend(["-f", ",".join(client_flags)])
        command.extend(["-t", session])
        return subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )

    def _read_stdout(self) -> None:
        assert self._process.stdout is not None
        for line in self._process.stdout:
            self._queue.put(line.rstrip("\n"))
        self._queue.put(None)

    def close(self) -> None:
        if self._process.poll() is not None:
            return
        self._process.terminate()
        try:
            self._process.wait(timeout=1.0)
        except subprocess.TimeoutExpired:
            self._process.kill()
            self._process.wait(timeout=1.0)

    def __enter__(self) -> "ControlModeClient":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def _get_line(self, timeout: float | None) -> str | None:
        if self._buffer:
            return self._buffer.popleft()
        try:
            return self._queue.get(timeout=timeout)
        except queue.Empty:
            return None

    def drain_lines(self, timeout: float = 0.5, quiet_period: float = 0.15) -> list[str]:
        lines: list[str] = []
        deadline = time.monotonic() + timeout
        quiet_deadline: float | None = None

        while time.monotonic() < deadline:
            remaining = deadline - time.monotonic()
            line = self._get_line(timeout=min(quiet_period, max(remaining, 0.0)))
            if line is None:
                if lines and quiet_deadline is not None and time.monotonic() >= quiet_deadline:
                    break
                continue
            lines.append(line)
            quiet_deadline = time.monotonic() + quiet_period

        return lines

    def notifications(self, lines: list[str]) -> list[str]:
        return [
            line
            for line in lines
            if line.startswith("%")
            and not line.startswith("%begin ")
            and not line.startswith("%end ")
            and not line.startswith("%error ")
        ]

    def command(self, command: str) -> None:
        assert self._process.stdin is not None
        self._process.stdin.write(command)
        self._process.stdin.write("\n")
        self._process.stdin.flush()

    def command_output(self, command: str, timeout: float = 2.0) -> list[str]:
        self.command(command)

        deadline = time.monotonic() + timeout
        block_tokens: list[str] | None = None
        output: list[str] = []

        while time.monotonic() < deadline:
            line = self._get_line(timeout=max(deadline - time.monotonic(), 0.01))
            if line is None:
                continue

            if block_tokens is None:
                if line.startswith("%begin "):
                    block_tokens = line.split()[1:4]
                else:
                    self._buffer.append(line)
                continue

            if line.startswith("%end ") or line.startswith("%error "):
                if line.split()[1:4] == block_tokens:
                    return output
                continue

            output.append(line)

        fail(f"timed out waiting for control-mode output from: {command}")
