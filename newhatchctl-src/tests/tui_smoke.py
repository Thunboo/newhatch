#!/usr/bin/env python3
"""Exercise the real TUI in a PTY with a fake Docker executable on PATH."""

import errno
import codecs
import fcntl
import json
import os
import pty
import re
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path


def _alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


class Screen:
    """Decode the cursor/clear sequences emitted by Ratatui's Crossterm backend."""

    def __init__(self):
        self.rows = [[" "] * 120 for _ in range(35)]
        self.row = self.column = 0
        self.pending = ""
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")

    def feed(self, data):
        text = self.pending + self.decoder.decode(data)
        self.pending = ""
        index = 0
        while index < len(text):
            character = text[index]
            if character == "\x1b":
                match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", text[index:])
                if not match:
                    self.pending = text[index:]
                    break
                parameters, _, command = match.groups()
                values = [int(value or 1) for value in parameters.split(";")] if not parameters.startswith("?") else []
                if command in {"H", "f"}:
                    self.row = min(34, (values[0] if values else 1) - 1)
                    self.column = min(119, (values[1] if len(values) > 1 else 1) - 1)
                elif command == "J" and parameters == "2":
                    self.rows = [[" "] * 120 for _ in range(35)]
                elif command == "K":
                    start = 0 if parameters in {"1", "2"} else self.column
                    end = self.column + 1 if parameters == "1" else 120
                    self.rows[self.row][start:end] = [" "] * (end - start)
                index += len(match.group())
                continue
            if character == "\r":
                self.column = 0
            elif character == "\n":
                self.row = min(34, self.row + 1)
            elif character.isprintable():
                self.rows[self.row][self.column] = character
                self.column = min(119, self.column + 1)
            index += 1

    def text(self):
        return "\n".join("".join(row) for row in self.rows)


binary = Path(sys.argv[1]).resolve()
repository = Path(__file__).resolve().parents[2]

with tempfile.TemporaryDirectory(prefix="newhatchctl-tui-") as temporary:
    root = Path(temporary)
    project = root / "newhatch"
    project.mkdir()
    shutil.copy(repository / "compose.yaml", project / "compose.yaml")
    env_file = project / ".env"
    env_file.write_text(
        "NEWHATCH_PASSWORD=fixture-password-never-display\n"
        "SEGMENT_DURATION=30m\n"
        + "".join(f"CUSTOM_{index}=value_{index}\n" for index in range(60))
        + "LAST_ENV_SETTING=before-refresh\n"
    )
    bin_dir = root / "bin"
    bin_dir.mkdir()
    shutil.copy(Path(__file__).with_name("fake_docker.py"), bin_dir / "docker")
    (bin_dir / "docker").chmod(0o755)
    env_values = {
        "CAPTURE_INTERFACE": "eth0", "FLAG_REGEX": r"FLAG\{[^}\r\n]+\}",
        "FRONTEND_PORT": "8080", "ANALYZER": "local", "AUTH_ALLOWED_SUBNETS": "10.0.0.0/8",
    }
    state_file = project / "fake-state.json"
    state_file.write_text(json.dumps({
        "name": "newhatch", "desired": {service: env_values for service in
            ["collector", "analyzer", "frontend", "suricata", "test-flag"]},
        "containers": {service: {"id": "id-" + service, "state": state, "env": env_values}
            for service, state in [("collector", "running"), ("analyzer", "exited"), ("suricata", "running")]},
        "images": ["collector", "analyzer"],
    }))
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 120, 0, 0))
    original_mode = termios.tcgetattr(slave)

    def terminal_session():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

    environment = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ.get("PATH", ""),
                       NEWHATCH_DIR=str(project), TERM="xterm-256color")
    process = subprocess.Popen([str(binary)], cwd=root, env=environment,
                               stdin=slave, stdout=slave, stderr=slave, preexec_fn=terminal_session)
    output = bytearray()
    screen = Screen()

    def pump():
        if select.select([master], [], [], 0.05)[0]:
            try:
                data = os.read(master, 65536)
                output.extend(data)
                screen.feed(data)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise

    def wait_for(predicate, description):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            pump()
            if predicate():
                return
            if process.poll() is not None:
                raise AssertionError(f"TUI exited while waiting for {description}:\n{screen.text()}")
        raise AssertionError(f"Timed out waiting for {description}:\n{screen.text()}")

    def send(keys):
        os.write(master, keys)

    def state():
        return json.loads(state_file.read_text())

    def mutations():
        calls = [json.loads(line) for line in (project / "calls.jsonl").read_text().splitlines()]
        return [args for args in calls if "up" in args or "stop" in args]

    def text_since(offset, text):
        return text in screen.text()

    try:
        wait_for(lambda: "FLAG_REGEX" in screen.text(), "loaded dashboard")
        send(b"\t\t\t")
        wait_for(lambda: "Home/End" in screen.text(), "settings focus")
        assert "NEWHATCH_PASSWORD = *****" in screen.text(), "Password mask missing"
        assert b"fixture-password-never-display" not in output, "Password leaked into terminal output"
        send(b"\x1b[F")
        wait_for(lambda: "LAST_ENV_SETTING" in screen.text(), "last .env setting")
        assert "before-refresh" in screen.text()
        env_file.write_text(env_file.read_text().replace("before-refresh", "after-refresh"))
        send(b"\x1b[15~")
        wait_for(lambda: "after-refresh" in screen.text(), "refreshed .env value")
        env_file.write_text("")
        send(b"\x1b[15~")
        wait_for(lambda: "FLAG_REGEX" in screen.text() and "LAST_ENV_SETTING" not in screen.text(),
                 "settings scroll clamped after file change")
        assert not mutations(), "viewing .env must not mutate containers"
        send(b"\x1b")
        wait_for(lambda: "component/scroll" in screen.text(), "component focus")
        offset = len(output)
        send(b"r")
        wait_for(lambda: text_since(offset, "Operation completed"), "collector recreation")
        assert len(mutations()) == 1 and "--force-recreate" in mutations()[0]
        assert state()["containers"]["analyzer"]["state"] == "exited"

        send(b"\x1b")
        wait_for(lambda: not (termios.tcgetattr(slave)[3] & termios.ICANON), "raw terminal")
        offset = len(output)
        send(b"\x1b[B\x1b[B" + b"s")
        wait_for(lambda: text_since(offset, "Start both containers?"), "frontend dependency prompt")
        send(b"\r")
        wait_for(lambda: "Cancelled" in screen.text(), "default No")
        assert len(mutations()) == 1

        offset = len(output)
        send(b"s")
        wait_for(lambda: text_since(offset, "Start both containers?"), "second dependency prompt")
        send(b"\t\r")
        wait_for(lambda: state()["containers"].get("frontend", {}).get("state") == "running"
                 and text_since(offset, "Operation completed"), "confirmed pair startup")
        assert [args[-1] for args in mutations()] == ["collector", "analyzer", "frontend"]

        send(b"\x1b")
        pump()
        offset = len(output)
        send(b"\x1b[A" + b"x")
        wait_for(lambda: text_since(offset, "Stop both containers?"), "analyzer stop prompt")
        send(b"\t\r")
        wait_for(lambda: state()["containers"]["analyzer"]["state"] == "exited"
                 and text_since(offset, "Operation completed"), "confirmed pair shutdown")
        assert [args[-1] for args in mutations()][-2:] == ["frontend", "analyzer"]

        send(b"\x1b")
        pump()
        offset = len(output)
        send(b"\x1b[A" + b"l")
        wait_for(lambda: text_since(offset, "fixture live log"), "live logs")
        assert "\\u{1b}" not in screen.text(), "ANSI escapes leaked into log text"
        assert "[32m" not in screen.text(), "Color parameters leaked into log text"
        pid = int((project / "log-pid").read_text())
        send(b"\x1b[5~\x1b[F\x1b")
        wait_for(lambda: not _alive(pid), "log reader cleanup")
        assert state()["containers"]["collector"]["state"] == "running"
        assert state()["containers"]["suricata"]["state"] == "running"
        send(b"q")
        deadline = time.monotonic() + 5
        while process.poll() is None and time.monotonic() < deadline:
            pump()
        assert process.poll() == 0, "TUI did not exit successfully"
        pump()
        assert b"\x1b[?1049l" in output, "Alternate screen was not restored"
        assert b"fixture-password-never-display" not in output, "Password leaked into terminal output"
        try:
            restored_mode = termios.tcgetattr(slave)
        except termios.error as error:
            # macOS revokes a controlling PTY when its session leader exits.
            # Successful exit and alternate-screen teardown are still verified.
            if error.args[0] != errno.ENOTTY:
                raise
        else:
            assert restored_mode[3] & termios.ICANON == original_mode[3] & termios.ICANON
        print("PASS: full .env settings, scrolling/refresh, keyboard TUI, recreation, dependency dialogs, live logs, process cleanup and terminal restoration")
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        pid_file = project / "log-pid"
        if pid_file.exists():
            pid = int(pid_file.read_text())
            if _alive(pid):
                os.kill(pid, signal.SIGKILL)
        os.close(master)
        os.close(slave)
