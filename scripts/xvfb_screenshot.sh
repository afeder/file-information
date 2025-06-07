#!/bin/bash
set -euo pipefail

XVFB_DISPLAY=:99
APP_PATH="target/release/file-information"
SCREENSHOT="/tmp/file_information_test_screenshot.png"
TEST_DIR="$HOME/tmp"
TEST_FILE="$TEST_DIR/testfile.txt"
XVFB_LOG="/tmp/xvfb.log"
app_pid=""
xvfb_pid=""

cleanup() {
    if [ -n "${app_pid:-}" ]; then
        kill "${app_pid}" 2>/dev/null || true
    fi
    if [ -n "${xvfb_pid:-}" ]; then
        kill "${xvfb_pid}" 2>/dev/null || true
    fi
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

echo "Launching Xvfb on display $XVFB_DISPLAY and piping output to $XVFB_LOG..."
Xvfb "$XVFB_DISPLAY" -screen 0 1024x768x24 >"$XVFB_LOG" 2>&1 &
xvfb_pid=$!

export DISPLAY="$XVFB_DISPLAY"
export GDK_BACKEND=x11
export GTK_A11Y=none
export LIBGL_ALWAYS_SOFTWARE=1

# Start a D-Bus session if needed.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    addr=$(dbus-daemon --session --fork --print-address)
    export DBUS_SESSION_BUS_ADDRESS="$addr"
fi

echo "Initiating Tracker indexing of $TEST_DIR..."
# Let Tracker index the test directory.
tracker3 daemon -s >/dev/null
tracker3 index --add "$TEST_DIR" >/dev/null

# Wait for the test file to be indexed before launching the application
echo "Waiting up to 60 seconds for Tracker to index $TEST_FILE..." >&2
for i in {1..60}; do
    if ! tracker3 info "$TEST_FILE" 2>&1 | grep -q "No metadata available"; then
        break
    fi
    sleep 1
done
if tracker3 info "$TEST_FILE" 2>&1 | grep -q "No metadata available"; then
    echo "Timed out waiting for Tracker to index $TEST_FILE." >&2
    exit 1
fi

# Query Tracker for metadata about the file to be shown in the application
echo "Tracker metadata for $TEST_FILE:" >&2
(tracker3 info "$TEST_FILE" || true) | head -n 5


"$APP_PATH" --debug "$TEST_FILE" &
app_pid=$!

echo "Waiting up to 60 seconds for the File Information window to be created..." >&2
for i in {1..60}; do
    if xdotool search --name "File Information" >/dev/null 2>&1; then
        break
    fi
    sleep 1
done
if ! xdotool search --name "File Information" >/dev/null 2>&1; then
    echo "Timed out waiting for the File Information window to be created." >&2
    exit 1
fi

window_id=$(xdotool search --name "File Information" | head -n 1)

# Wait for the window to be fully drawn before taking a screenshot. The window
# can exist before it has finished rendering, resulting in a blank capture. Use
# xwininfo to check that the map state is "IsViewable" and give the GUI a bit of
# extra time to paint.
echo "Waiting up to 10 seconds for the File Information window to become viewable..." >&2
for i in {1..20}; do
    if xwininfo -id "$window_id" | grep -q "IsViewable"; then
        break
    fi
    sleep 0.5
done
if ! xwininfo -id "$window_id" | grep -q "IsViewable"; then
    echo "Timed out waiting for the File Information window to become viewable." >&2
    exit 1
fi
sleep 1

echo "Saves screenshot of window $window_id on display $XVFB_DISPLAY to $SCREENSHOT..."
import -display "$XVFB_DISPLAY" -window "$window_id" "$SCREENSHOT"

# Print geometry using the captured window ID
echo "Acquiring window geometry..."
geom=$(xdotool getwindowgeometry --shell "$window_id")
eval "$geom"
echo "Window geometry: X=$X Y=$Y WIDTH=$WIDTH HEIGHT=$HEIGHT" >&2

# Click the "Close" button near the lower-right corner of the window
close_x=$((X + WIDTH - 20))
close_y=$((Y + HEIGHT - 20))
xdotool mousemove --sync "$close_x" "$close_y" click 1

# Check if the window closed successfully
sleep 1
if xwininfo -id "$window_id" >/dev/null 2>&1; then

    echo "Window did not close." >&2
else
    echo "Window closed successfully." >&2
fi

exit 0
