#!/usr/bin/env python3

import argparse
import sys
import time


ESC = "\x1b"
BEL = "\x07"
ST = f"{ESC}\\"


def emit(label: str, payload: str, pause: float) -> None:
    sys.stdout.write(f"\nBEGIN:{label}\n")
    sys.stdout.flush()
    sys.stdout.write(payload)
    sys.stdout.flush()
    time.sleep(pause)
    sys.stdout.write(f"\nEND:{label}\n")
    sys.stdout.flush()
    time.sleep(pause)


def emit_parts(label: str, parts: list[str], pause: float) -> None:
    sys.stdout.write(f"\nBEGIN:{label}\n")
    sys.stdout.flush()
    for part in parts:
        sys.stdout.write(part)
        sys.stdout.flush()
        time.sleep(pause)
    sys.stdout.write(f"\nEND:{label}\n")
    sys.stdout.flush()
    time.sleep(pause)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pause", type=float, default=0.1)
    args = parser.parse_args()

    samples = [
        ("plain-text", "plain-text-line\n"),
        ("osc0-bel", f"{ESC}]0;osc0-bell-title{BEL}"),
        ("osc0-st", f"{ESC}]0;osc0-st-title{ST}"),
        ("osc7-bel", f"{ESC}]7;file:///tmp/osc-seven{BEL}"),
        ("osc7-st", f"{ESC}]7;file:///tmp/osc-seven-st{ST}"),
        ("osc8-bel", f"{ESC}]8;;https://example.com/osc8{BEL}osc8-link{ESC}]8;;{BEL}"),
        ("osc8-st", f"{ESC}]8;;https://example.com/osc8-st{ST}osc8-link-st{ESC}]8;;{ST}"),
        ("osc9-bel", f"{ESC}]9;osc9-bell-message{BEL}"),
        ("osc9-st", f"{ESC}]9;osc9-st-message{ST}"),
        ("bel-byte", BEL),
    ]

    for label, payload in samples:
        emit(label, payload, args.pause)

    emit_parts(
        "osc9-fragmented-bel",
        [f"{ESC}]9;", "osc9-fragmented-", f"message{BEL}"],
        args.pause,
    )
    emit_parts(
        "osc9-fragmented-st",
        [f"{ESC}]9;", "osc9-fragmented-st-", f"message{ST}"],
        args.pause,
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
