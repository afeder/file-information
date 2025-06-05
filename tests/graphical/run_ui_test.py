#!/usr/bin/env python3
import subprocess
import tempfile
import os
import time
import signal
import re


def wait_for_window(title, timeout=10):
    end = time.time() + timeout
    while time.time() < end:
        try:
            out = subprocess.check_output([
                "xdotool",
                "search",
                "--name",
                title,
            ])
            return out.split()[0].decode()
        except subprocess.CalledProcessError:
            time.sleep(0.5)
    raise RuntimeError(f"window '{title}' not found")

def main():
    subprocess.check_call(["cargo", "build"])
    fd, path = tempfile.mkstemp()
    os.close(fd)

    env = os.environ.copy()
    env["LIBGL_ALWAYS_SOFTWARE"] = "1"
    env["GTK_A11Y"] = "none"

    display = ":99"
    xvfb = subprocess.Popen([
        "Xvfb",
        display,
        "-screen",
        "0",
        "1024x768x24",
    ])
    env["DISPLAY"] = display
    os.environ["DISPLAY"] = display
    time.sleep(1)

    proc = subprocess.Popen([
        "target/debug/file-information",
        path,
    ], env=env)
    try:
        win_id = wait_for_window("File Information")
        geom = subprocess.check_output(["xdotool", "getwindowgeometry", win_id]).decode()
        m = re.search(r"Geometry: (\d+)x(\d+)", geom)
        width, height = map(int, m.groups()) if m else (590, 400)
        x = width - 30
        y = height - 15
        subprocess.check_call(["xdotool", "mousemove", "--window", win_id, str(x), str(y)])
        subprocess.check_call(["xdotool", "click", "--window", win_id, "1"])
        proc.wait(timeout=5)
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        xvfb.terminate()
        xvfb.wait()
        os.remove(path)
    if proc.returncode not in (0, -signal.SIGTERM):
        raise SystemExit(proc.returncode)

if __name__ == "__main__":
    main()
