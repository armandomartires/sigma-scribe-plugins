#!/usr/bin/env python3
"""Fail if toolchain pins drift, or if a cargo compile step in CI lacks --locked.

Plan item P1-05: rust-toolchain.toml, the GitLab Rust image tag, and
RUSTUP_TOOLCHAIN must stay in lock-step. cargo build/test/check/clippy/doc/run
invocations in CI must pass --locked. cargo fmt and cargo deny are exempt.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def fail(msg: str) -> None:
    print(f"error: {msg}", file=sys.stderr)
    raise SystemExit(1)


def rust_toolchain_channel() -> str:
    text = (ROOT / "rust-toolchain.toml").read_text()
    m = re.search(r'(?m)^channel\s*=\s*"([^"]+)"', text)
    if not m:
        fail("no channel in rust-toolchain.toml")
    return m.group(1)


def gitlab_variable(name: str) -> str | None:
    """Return the first top-level (two-space) YAML assignment of name."""
    text = (ROOT / ".gitlab-ci.yml").read_text()
    m = re.search(rf"(?m)^  {re.escape(name)}:\s*(.+)$", text)
    if not m:
        return None
    val = m.group(1).strip()
    if val.startswith(("'", '"')) and val.endswith(val[0]) and len(val) >= 2:
        val = val[1:-1]
    return val


def cargo_toml_rust_version() -> str | None:
    cargo = ROOT / "Cargo.toml"
    if not cargo.is_file():
        return None
    m = re.search(r'(?m)^rust-version\s*=\s*"([^"]+)"', cargo.read_text())
    return m.group(1) if m else None


def check_toolchain_pins() -> None:
    channel = rust_toolchain_channel()
    print(f"rust-toolchain.toml channel = {channel}")

    image = gitlab_variable("RUST_IMAGE")
    if not image:
        fail("RUST_IMAGE is not set at the top level of .gitlab-ci.yml")
    m = re.match(r"rust:([^@\s-]+)", image)
    if not m:
        fail(f"RUST_IMAGE is not a rust:<version>-... image: {image}")
    if m.group(1) != channel:
        fail(f"RUST_IMAGE version {m.group(1)} != rust-toolchain.toml {channel}")
    if "@sha256:" not in image:
        fail(f"RUST_IMAGE is not pinned by digest: {image}")
    print(f"RUST_IMAGE matches and is digest-pinned")

    toolchain = gitlab_variable("RUSTUP_TOOLCHAIN")
    if toolchain is None:
        fail("RUSTUP_TOOLCHAIN is not set at the top level of .gitlab-ci.yml")
    if toolchain != channel:
        fail(f"RUSTUP_TOOLCHAIN {toolchain} != rust-toolchain.toml {channel}")
    print(f"RUSTUP_TOOLCHAIN matches")

    rust_version = cargo_toml_rust_version()
    msrv_image = gitlab_variable("MSRV_IMAGE")
    if msrv_image:
        mm = re.match(r"rust:([^@\s-]+)", msrv_image)
        if not mm:
            fail(f"MSRV_IMAGE is not a rust:<version>-... image: {msrv_image}")
        if "@sha256:" not in msrv_image:
            fail(f"MSRV_IMAGE is not pinned by digest: {msrv_image}")
        if rust_version and not (
            mm.group(1) == rust_version or mm.group(1).startswith(rust_version + ".")
        ):
            fail(
                f"MSRV_IMAGE version {mm.group(1)} does not match "
                f"Cargo.toml rust-version {rust_version}"
            )
        print(f"MSRV_IMAGE {mm.group(1)} matches rust-version {rust_version}")
    elif rust_version:
        print(f"Cargo.toml rust-version = {rust_version} (no MSRV_IMAGE in CI)")


COMPILE = re.compile(
    r"cargo\s+(?:build|test|check|clippy|doc|run|bench|install)\b"
)


def iter_ci_files() -> list[Path]:
    files = [ROOT / ".gitlab-ci.yml"]
    workflows = ROOT / ".github" / "workflows"
    if workflows.is_dir():
        files.extend(sorted(workflows.glob("*.yml")))
        files.extend(sorted(workflows.glob("*.yaml")))
    scripts = ROOT / "scripts"
    if scripts.is_dir():
        # Local helper scripts (sim.sh) are not CI gates and may omit --locked.
        for sh in sorted(scripts.glob("*.sh")):
            if sh.name == "sim.sh":
                continue
            files.append(sh)
    return [p for p in files if p.is_file()]


def strip_yaml_comment(line: str) -> str:
    """Drop an unquoted # comment. Good enough for our CI files."""
    in_single = in_double = False
    for i, ch in enumerate(line):
        if ch == "'" and not in_double:
            in_single = not in_single
        elif ch == '"' and not in_single:
            in_double = not in_double
        elif ch == "#" and not in_single and not in_double:
            if i == 0 or line[i - 1].isspace():
                return line[:i].rstrip()
    return line.rstrip()


def commands_from(text: str) -> list[tuple[int, str]]:
    """Yield (start_line, command) for cargo compile invocations.

    A command starts on a line that contains a compile subcommand and
    continues while later lines are indented further (YAML folded argv
    or shell continuations).
    """
    raw_lines = text.splitlines()
    lines = [strip_yaml_comment(l) for l in raw_lines]
    out: list[tuple[int, str]] = []
    i = 0
    while i < len(lines):
        if not COMPILE.search(lines[i]):
            i += 1
            continue
        start = i
        indent = len(lines[i]) - len(lines[i].lstrip(" "))
        parts = [lines[i].strip().removesuffix("\\").strip()]
        i += 1
        while i < len(lines):
            s = lines[i]
            if not s.strip():
                break
            next_indent = len(s) - len(s.lstrip(" "))
            continued = s.rstrip().endswith("\\") or next_indent > indent
            if not continued:
                break
            parts.append(s.strip().lstrip("-").strip().removesuffix("\\").strip())
            i += 1
        out.append((start + 1, " ".join(p for p in parts if p)))
    return out


def check_locked() -> None:
    errors = []
    for path in iter_ci_files():
        rel = path.relative_to(ROOT)
        for lineno, cmd in commands_from(path.read_text()):
            if "--locked" in cmd:
                print(f"ok  {rel}:{lineno}: {cmd[:100]}")
            else:
                errors.append(f"{rel}:{lineno}: missing --locked: {cmd}")
    if errors:
        for e in errors:
            print(f"error: {e}", file=sys.stderr)
        raise SystemExit(1)


def main() -> None:
    check_toolchain_pins()
    check_locked()
    print("all CI toolchain and --locked pins are consistent")


if __name__ == "__main__":
    main()
