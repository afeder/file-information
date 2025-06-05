#!/bin/bash
set -euo pipefail

# Note that it may be necessary to install `dbus-x11` to avoid warnings about
# the session bus.

XVFB_DISPLAY=:99

cleanup() {
    kill "${APP_PID}" 2>/dev/null || true
    kill "${XVFB_PID}" 2>/dev/null || true
}
trap cleanup EXIT

if [ ! -x target/release/file-information ]; then
    cargo build --release
fi

Xvfb "$XVFB_DISPLAY" -screen 0 1024x768x24 &
XVFB_PID=$!

sleep 2
export DISPLAY="$XVFB_DISPLAY"
# Force GTK apps to use the X11 backend so they don't try to default to Wayland
# where available instead of connecting to Xvfb.
export GDK_BACKEND=x11
# This may be necessary to prevent attempts to use hardware graphics.
#export LIBGL_ALWAYS_SOFTWARE=1

./target/release/file-information README.md &
APP_PID=$!

FOUND=1
for i in {1..10}; do
    if xdotool search --name "File Information" >/dev/null 2>&1; then
        FOUND=0
        break
    fi
    sleep 1
done

exit $FOUND
