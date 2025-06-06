#!/bin/bash
set -euo pipefail

XVFB_DISPLAY=:99
APP_PATH="target/release/file-information"
SCREENSHOT="/tmp/file_information_test_screenshot.png"
TEST_DIR="$HOME/tmp"
TEST_FILE="$TEST_DIR/testfile.txt"

cleanup() {
    kill "${APP_PID}" 2>/dev/null || true
    kill "${XVFB_PID}" 2>/dev/null || true
}
trap cleanup EXIT

echo "Building the application (may take some time)..."
if [ ! -x "$APP_PATH" ]; then
    cargo build --release
fi

# Create the directory and test file so Tracker can index it
mkdir -p "$TEST_DIR"
if [ ! -f "$TEST_FILE" ]; then
    echo "This is a Tracker test file" > "$TEST_FILE"
fi

Xvfb "$XVFB_DISPLAY" -screen 0 1024x768x24 &
XVFB_PID=$!

sleep 2
export DISPLAY="$XVFB_DISPLAY"
export GDK_BACKEND=x11
export GTK_A11Y=none
export LIBGL_ALWAYS_SOFTWARE=1

# Start a D-Bus session if needed.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    addr=$(dbus-daemon --session --fork --print-address)
    export DBUS_SESSION_BUS_ADDRESS="$addr"
fi

#  Let Tracker to index the test directory.
tracker3 daemon -s >/dev/null
tracker3 index --add "$TEST_DIR" >/dev/null

# Wait for the test file to be indexed before launching the application
echo "Waiting for Tracker to index $TEST_FILE..." >&2
while tracker3 info "$TEST_FILE" 2>&1 | grep -q "No metadata available"; do
    sleep 1
done

# Query Tracker for metadata about the file to be shown in the application
echo "Tracker metadata for $TEST_FILE:" >&2
tracker3 info "$TEST_FILE" || true

"$APP_PATH" --debug "$TEST_FILE" &
APP_PID=$!

for i in {1..10}; do
    if xdotool search --name "File Information" >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

# Allow the application time to render its UI fully before taking the screenshot
sleep 2

import -display "$XVFB_DISPLAY" -window root "$SCREENSHOT"

# Print geometry of the "File Information" window
geom=$(xdotool search --name "File Information" getwindowgeometry --shell)
eval "$geom"
echo "Window geometry: X=$X Y=$Y WIDTH=$WIDTH HEIGHT=$HEIGHT" >&2

# Click the "Close" button near the lower-right corner of the window
close_x=$((X + WIDTH - 20))
close_y=$((Y + HEIGHT - 20))
xdotool mousemove --sync "$close_x" "$close_y" click 1

# Check if the window closed successfully
sleep 1
if xdotool search --name "File Information" >/dev/null 2>&1; then
    echo "Window did not close" >&2
else
    echo "Window closed successfully" >&2
fi

exit 0
