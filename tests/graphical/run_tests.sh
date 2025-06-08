#!/bin/bash
set -euo pipefail

XVFB_DISPLAY=:99
SCREENSHOT="/tmp/file_information_test_screenshot.png"
MASKED_SCREENSHOT="/tmp/file_information_test_screenshot_masked.png"
BACKLINKS_SCREENSHOT="/tmp/file_information_backlinks_screenshot.png"
BACKLINKS_MASKED_SCREENSHOT="/tmp/file_information_backlinks_screenshot_masked.png"
TEST_DIR="$HOME/tmp"
TEST_FILE="$TEST_DIR/testfile.txt"
XVFB_LOG="/tmp/xvfb.log"
APP_LOG="/tmp/file_information_app.log"
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

release=false
for arg in "$@"; do
    if [ "$arg" = "--release" ]; then
        release=true
        break
    fi
done

if $release; then
    app_path="target/release/file-information"
else
    app_path="target/debug/file-information"
fi

echo "Building the application (may take some time)..."
if [ ! -x "$app_path" ]; then
    if $release; then
        cargo build --release
    else
        cargo build
    fi
fi

# Create the directory and test file so Tracker can index it.
mkdir -p "$TEST_DIR"
if [ ! -f "$TEST_FILE" ]; then
    echo "The quick brown fox jumps over the lazy dog." > "$TEST_FILE"
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

# Wait for the test file to be indexed before launching the application.
echo "Waiting up to 60 seconds for Tracker to index $TEST_FILE..." >&2
for i in {1..600}; do
    if ! tracker3 info "$TEST_FILE" 2>&1 | grep -q "No metadata available"; then
        break
    fi
    sleep 0.1
done
if tracker3 info "$TEST_FILE" 2>&1 | grep -q "No metadata available"; then
    echo "Timed out waiting for Tracker to index $TEST_FILE." >&2
    exit 1
fi

# Query Tracker for metadata about the file to be shown in the application.
echo "Tracker metadata for $TEST_FILE:" >&2
(tracker3 info "$TEST_FILE" || true) | head -n 5


rm -f "$APP_LOG"
"$app_path" --debug "$TEST_FILE" >"$APP_LOG" 2>&1 &
app_pid=$!

echo "Waiting up to 10 seconds for the File Information window to be created..." >&2
for i in {1..100}; do
    if xdotool search --name "File Information" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if ! xdotool search --name "File Information" >/dev/null 2>&1; then
    echo "Timed out waiting for the File Information window to be created." >&2
    exit 1
fi

window_id=$(xdotool search --name "File Information" | head -n 1)

# Wait for the window to be fully drawn before taking a screenshot. The window
# can exist before it has finished rendering, resulting in a blank capture. Use
# xwininfo to wait until the map state is "IsViewable".
echo "Waiting up to 10 seconds for the File Information window to become viewable..." >&2
for i in {1..100}; do
    if xwininfo -id "$window_id" | grep -q "IsViewable"; then
        break
    fi
    sleep 0.1
done
if ! xwininfo -id "$window_id" | grep -q "IsViewable"; then
    echo "Timed out waiting for the File Information window to become viewable." >&2
    exit 1
fi

echo "Waiting up to 10 seconds for results to be displayed..." >&2
ready=false
# Poll for the structured debug message that indicates results are visible.
for i in {1..100}; do
    if grep -q "DEBUG: results displayed" "$APP_LOG"; then
        ready=true
        break
    fi
    sleep 0.1
done
if ! $ready; then
    echo "Timed out waiting for results to be displayed." >&2
    exit 1
fi

echo "Saving screenshot of window $window_id on display $XVFB_DISPLAY to $SCREENSHOT..."
import -display "$XVFB_DISPLAY" -window "$window_id" "$SCREENSHOT"

# Mask variable regions that can affect the MD5 digest by overlaying black
# rectangles on the captured image.
convert "$SCREENSHOT" \
    -fill black -draw "rectangle 176,130 310,143" \
    -fill black -draw "rectangle 176,180 310,193" \
    -fill black -draw "rectangle 176,205 183,218" \
    -fill black -draw "rectangle 11,230 568,445" \
    "$MASKED_SCREENSHOT"

# Compute and log the MD5 digest of the raw screenshot so it can be compared
# against known values.
digest=$(convert "$MASKED_SCREENSHOT" rgba:- | md5sum | awk '{print $1}')
echo "Screenshot MD5 digest: $digest" >&2

# Print geometry using the captured window ID.
echo "Acquiring window geometry for window $window_id..."
geom=$(xdotool getwindowgeometry --shell "$window_id")
eval "$geom"
echo "Window geometry: X=$X Y=$Y WIDTH=$WIDTH HEIGHT=$HEIGHT" >&2

# Save main window geometry for later interactions.
main_X=$X
main_Y=$Y
main_WIDTH=$WIDTH
main_HEIGHT=$HEIGHT

# Open the Backlinks view by clicking the Backlinks button near the bottom-right
# of the main window.
backlinks_x=$((main_X + main_WIDTH - 270))
backlinks_y=$((main_Y + main_HEIGHT - 20))
xdotool mousemove --sync "$backlinks_x" "$backlinks_y" click 1

echo "Waiting up to 10 seconds for the Backlinks window to be created..." >&2
for i in {1..100}; do
    if xdotool search --name "Backlinks" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if ! xdotool search --name "Backlinks" >/dev/null 2>&1; then
    echo "Timed out waiting for the Backlinks window to be created." >&2
    exit 1
fi

back_window_id=$(xdotool search --name "Backlinks" | head -n 1)

echo "Waiting up to 10 seconds for backlinks to be displayed..." >&2
back_ready=false
for i in {1..100}; do
    if grep -q "Backlinks query returned" "$APP_LOG"; then
        back_ready=true
        break
    fi
    sleep 0.1
done
if ! $back_ready; then
    echo "Timed out waiting for backlinks to be displayed." >&2
    exit 1
fi

echo "Saving screenshot of window $back_window_id on display $XVFB_DISPLAY to $BACKLINKS_SCREENSHOT..."
import -display "$XVFB_DISPLAY" -window "$back_window_id" "$BACKLINKS_SCREENSHOT"

# Mask variable regions in the Backlinks screenshot.
convert "$BACKLINKS_SCREENSHOT" \
    -fill black -draw "rectangle 176,130 310,143" \
    -fill black -draw "rectangle 176,180 310,193" \
    -fill black -draw "rectangle 176,205 183,218" \
    -fill black -draw "rectangle 11,230 568,445" \
    "$BACKLINKS_MASKED_SCREENSHOT"

back_digest=$(convert "$BACKLINKS_MASKED_SCREENSHOT" rgba:- | md5sum | awk '{print $1}')
echo "Backlinks screenshot MD5 digest: $back_digest" >&2

echo "Acquiring window geometry for window $back_window_id..."
geom=$(xdotool getwindowgeometry --shell "$back_window_id")
eval "$geom"
echo "Backlinks window geometry: X=$X Y=$Y WIDTH=$WIDTH HEIGHT=$HEIGHT" >&2

# Close the Backlinks window.
close_x=$((X + WIDTH - 20))
close_y=$((Y + HEIGHT - 20))
xdotool mousemove --sync "$close_x" "$close_y" click 1

echo "Waiting up to 10 seconds for the Backlinks window to close..." >&2
closed=false
for i in {1..100}; do
    if ! xwininfo -id "$back_window_id" >/dev/null 2>&1; then
        closed=true
        break
    fi
    sleep 0.1
done
if $closed; then
    echo "Backlinks window closed successfully." >&2
else
    echo "Backlinks window failed to close." >&2
    exit 1
fi

# Now close the main window.
main_close_x=$((main_X + main_WIDTH - 20))
main_close_y=$((main_Y + main_HEIGHT - 20))
xdotool mousemove --sync "$main_close_x" "$main_close_y" click 1

echo "Waiting up to 10 seconds for the main window to close..." >&2
closed=false
for i in {1..100}; do
    if ! xwininfo -id "$window_id" >/dev/null 2>&1; then
        closed=true
        break
    fi
    sleep 0.1
done
if $closed; then
    echo "Window closed successfully." >&2
else
    echo "Window did not close." >&2
fi

exit 0
