#!/bin/bash
set -euo pipefail

XVFB_DISPLAY=:99
MAIN_SCREENSHOT="/tmp/file_information_main_screenshot.png"
MAIN_SCREENSHOT_MASKED="/tmp/file_information_main_screenshot_masked.png"
MAIN_SCREENSHOT_STORED="tests/graphical/images/file_information_main_screenshot.png"
MAIN_SCREENSHOT_STORED_MASKED="/tmp/file_information_main_screenshot_masked_stored.png"
BACKLINKS_SCREENSHOT="/tmp/file_information_backlinks_screenshot.png"
BACKLINKS_SCREENSHOT_MASKED="/tmp/file_information_backlinks_screenshot_masked.png"
BACKLINKS_SCREENSHOT_STORED="tests/graphical/images/file_information_backlinks_screenshot.png"
BACKLINKS_SCREENSHOT_STORED_MASKED="/tmp/file_information_backlinks_screenshot_masked_stored.png"
TEST_DIR="$HOME/tmp"
TEST_FILE="$TEST_DIR/testfile.txt"
XVFB_LOG="/tmp/xvfb.log"
APP_LOG="/tmp/file_information_app.log"

# ANSI color codes for log messages.
GREEN="\033[1;32m"
YELLOW="\033[1;33m"
RED="\033[1;31m"
RESET="\033[0m"

app_pid=""
xvfb_pid=""

cleanup() {
    if [ -n "${app_pid:-}" ]; then
        kill "${app_pid}" 2>/dev/null || true
    fi
    if [ -n "${xvfb_pid:-}" ]; then
        kill "${xvfb_pid}" 2>/dev/null || true
    fi
    if [ -n "${TEMP_XDG_HOME:-}" ] && [ -d "$TEMP_XDG_HOME" ]; then
        rm -rf "$TEMP_XDG_HOME"
    fi
}
trap cleanup EXIT

# Log a message with a timestamp and colored text.
log() {
    printf "%b[%s]%b %b%s%b\n" "$GREEN" "$(date '+%H:%M:%S')" "$RESET" "$YELLOW" "$*" "$RESET" >&2
}

# Log an error message in red with a timestamp.
error() {
    printf "%b[%s]%b %b%s%b\n" "$GREEN" "$(date '+%H:%M:%S')" "$RESET" "$RED" "$*" "$RESET" >&2
}

# Helper to measure command duration in milliseconds.
run_and_time() {
    local start end duration
    start=$(date +%s%N)
    "$@"
    end=$(date +%s%N)
    duration=$(( (end - start) / 1000000 ))
    log "Command '$*' took ${duration} ms."
}

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

log "Building the application..."
if [ ! -x "$app_path" ]; then
    if $release; then
        cargo build --release
    else
        cargo build
    fi
    log "Build complete. Binary located at $app_path."
else
    log "Using existing build at $app_path."
fi

# Create the directory and test file so Tracker can index it.
log "Creating test file $TEST_FILE..."
mkdir -p "$TEST_DIR"
if [ ! -f "$TEST_FILE" ]; then
    echo "The quick brown fox jumps over the lazy dog." > "$TEST_FILE"
fi

log "Launching Xvfb on display $XVFB_DISPLAY and piping output to $XVFB_LOG..."
Xvfb "$XVFB_DISPLAY" -screen 0 1024x768x24 >"$XVFB_LOG" 2>&1 &
xvfb_pid=$!
log "Xvfb started with PID $xvfb_pid."

export DISPLAY="$XVFB_DISPLAY"
export GDK_BACKEND=x11
export GTK_A11Y=none
export LIBGL_ALWAYS_SOFTWARE=1

# Set up a mock default handler for opening text files. This must be
# completely isolated from the user's real configuration.
TEMP_XDG_HOME="$(mktemp -d)"
export XDG_DATA_HOME="$TEMP_XDG_HOME"
export XDG_DATA_DIRS="$TEMP_XDG_HOME:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
MOCK_OPEN_SCRIPT="$TEMP_XDG_HOME/mock-open.sh"
MOCK_OPEN_DESKTOP="$TEMP_XDG_HOME/applications/mock-open.desktop"
log "Configuring mock default handler for text/plain in $TEMP_XDG_HOME ..."
cat <<'EOS' >"$MOCK_OPEN_SCRIPT"
#!/bin/sh
echo "$@" >>/tmp/mock_open_args
touch /tmp/mock_open_invoked
exit 0
EOS
chmod +x "$MOCK_OPEN_SCRIPT"
mkdir -p "$(dirname "$MOCK_OPEN_DESKTOP")"
cat <<EOS >"$MOCK_OPEN_DESKTOP"
[Desktop Entry]
Type=Application
Name=Mock Open
Exec=$MOCK_OPEN_SCRIPT %U
NoDisplay=true
MimeType=text/plain
EOS
update-desktop-database "$(dirname "$MOCK_OPEN_DESKTOP")" >/dev/null 2>&1 || true
xdg-mime default "$(basename "$MOCK_OPEN_DESKTOP")" text/plain

# Start a D-Bus session if needed.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    addr=$(dbus-daemon --session --fork --print-address)
    export DBUS_SESSION_BUS_ADDRESS="$addr"
fi

log "Initiating Tracker indexing of $TEST_DIR..."
# Let Tracker index the test directory.
tracker3 daemon -s >/dev/null
tracker3 index --add "$TEST_DIR" >/dev/null

# Wait for the test file to be indexed before launching the application.
log "Waiting up to 60 seconds for Tracker to index $TEST_FILE..."
for i in {1..600}; do
    if ! tracker3 info "$TEST_FILE" 2>&1 | grep -q "No metadata available"; then
        break
    fi
    sleep 0.1
done
if tracker3 info "$TEST_FILE" 2>&1 | grep -q "No metadata available"; then
    error "Timed out waiting for Tracker to index $TEST_FILE."
    exit 1
fi

# Query Tracker for metadata about the file to be shown in the application.
log "Tracker metadata for $TEST_FILE:"
(tracker3 info "$TEST_FILE" || true) | head -n 5


rm -f "$APP_LOG"
"$app_path" --debug "$TEST_FILE" >"$APP_LOG" 2>&1 &
app_pid=$!
log "Application started with PID $app_pid; logging to $APP_LOG."

log "Waiting up to 10 seconds for the main window to be created..."
for i in {1..100}; do
    if xdotool search --name "File Information" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if ! xdotool search --name "File Information" >/dev/null 2>&1; then
    error "Timed out waiting for the main window to be created."
    exit 1
fi

main_window_id=$(xdotool search --name "File Information" | head -n 1)
log "Main window ID acquired: $main_window_id."

# Wait for the window to be fully drawn before taking a screenshot. The window
# can exist before it has finished rendering, resulting in a blank capture. Use
# xwininfo to wait until the map state is "IsViewable".
log "Waiting up to 10 seconds for the main window to become viewable..."
for i in {1..100}; do
    if xwininfo -id "$main_window_id" | grep -q "IsViewable"; then
        break
    fi
    sleep 0.1
done
if ! xwininfo -id "$main_window_id" | grep -q "IsViewable"; then
    error "Timed out waiting for the main window to become viewable."
    exit 1
fi

log "Waiting up to 10 seconds for query results to be displayed..."
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
    error "Timed out waiting for query results to be displayed."
    exit 1
fi

log "Saving screenshot of window $main_window_id on display $XVFB_DISPLAY to $MAIN_SCREENSHOT..."
import -display "$XVFB_DISPLAY" -window "$main_window_id" "$MAIN_SCREENSHOT"
log "Main window screenshot saved to $MAIN_SCREENSHOT."

# Mask known variable regions that can affect the MD5 digest by overlaying black
# rectangles on the captured image.
convert "$MAIN_SCREENSHOT" \
    -fill black -draw "rectangle 176,130 310,143" \
    -fill black -draw "rectangle 176,180 310,193" \
    -fill black -draw "rectangle 176,205 183,218" \
    -fill black -draw "rectangle 11,230 568,445" \
    "$MAIN_SCREENSHOT_MASKED"

# Compute and log the MD5 digest of the raw screenshot so it can be compared
# against known values.
main_window_digest=$(convert "$MAIN_SCREENSHOT_MASKED" rgba:- | md5sum | awk '{print $1}')
log "Masked main window screenshot MD5 digest: $main_window_digest."

# Compute MD5 digest of the stored reference screenshot for comparison.
convert "$MAIN_SCREENSHOT_STORED" \
    -fill black -draw "rectangle 176,130 310,143" \
    -fill black -draw "rectangle 176,180 310,193" \
    -fill black -draw "rectangle 176,205 183,218" \
    -fill black -draw "rectangle 11,230 568,445" \
    "$MAIN_SCREENSHOT_STORED_MASKED"
main_window_stored_digest=$(convert "$MAIN_SCREENSHOT_STORED_MASKED" rgba:- | md5sum | awk '{print $1}')
if [ "$main_window_digest" = "$main_window_stored_digest" ]; then
    log "Masked main window screenshot matches the stored reference."
else
    error "Masked main window screenshot does not match the stored reference."
fi

# Print geometry using the captured window ID.
log "Acquiring window geometry for window $main_window_id..."
geom=$(xdotool getwindowgeometry --shell "$main_window_id")
eval "$geom"
log "Window geometry: X=$X Y=$Y WIDTH=$WIDTH HEIGHT=$HEIGHT."

# Save main window geometry for later interactions.
main_X=$X
main_Y=$Y
main_WIDTH=$WIDTH
main_HEIGHT=$HEIGHT

# Click the "Open" button and verify our mock handler is executed.
log "Clicking the \"Open\" button in the main window..."
rm -f /tmp/mock_open_invoked /tmp/mock_open_args
open_x=$((main_X + main_WIDTH - 150))
open_y=$((main_Y + main_HEIGHT - 20))
run_and_time xdotool mousemove --sync "$open_x" "$open_y" click 1

for i in {1..100}; do
    if [ -f /tmp/mock_open_invoked ]; then
        log "The mock handler was successfully triggered when clicking \"Open\"."
        break
    fi
    sleep 0.1
done
if [ ! -f /tmp/mock_open_invoked ]; then
    error "Clicking the \"Open\" button failed to trigger the mock handler."
fi

# Open the "Backlinks" view by clicking the "Backlinks" button near the bottom-right
# of the main window.
log "Clicking the \"Backlinks\" button in the main window..."
backlinks_x=$((main_X + main_WIDTH - 270))
backlinks_y=$((main_Y + main_HEIGHT - 20))
run_and_time xdotool mousemove --sync "$backlinks_x" "$backlinks_y" click 1

log "Waiting up to 10 seconds for the \"Backlinks\" window to be created..."
for i in {1..100}; do
    if xdotool search --name "Backlinks" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if ! xdotool search --name "Backlinks" >/dev/null 2>&1; then
    error "Timed out waiting for the \"Backlinks\" window to be created."
    exit 1
fi

backlinks_window_id=$(xdotool search --name "Backlinks" | head -n 1)
log "Backlinks window ID acquired: $backlinks_window_id."

log "Waiting up to 10 seconds for query results to be displayed..."
back_ready=false
for i in {1..100}; do
    if grep -q "Backlinks query returned" "$APP_LOG"; then
        back_ready=true
        break
    fi
    sleep 0.1
done
if ! $back_ready; then
    error "Timed out waiting for query results to be displayed."
    exit 1
fi

log "Saving screenshot of window $backlinks_window_id on display $XVFB_DISPLAY to $BACKLINKS_SCREENSHOT..."
import -display "$XVFB_DISPLAY" -window "$backlinks_window_id" "$BACKLINKS_SCREENSHOT"
log "\"Backlinks\" window screenshot saved to $BACKLINKS_SCREENSHOT."

# Mask known variable regions in the "Backlinks" window screenshot.
convert "$BACKLINKS_SCREENSHOT" \
    -fill black -draw "rectangle 11,57 310,70" \
    "$BACKLINKS_SCREENSHOT_MASKED"

backlinks_window_digest=$(convert "$BACKLINKS_SCREENSHOT_MASKED" rgba:- | md5sum | awk '{print $1}')
log "Masked \"Backlinks\" window screenshot MD5 digest: $backlinks_window_digest."

convert "$BACKLINKS_SCREENSHOT_STORED" \
    -fill black -draw "rectangle 11,57 310,70" \
    "$BACKLINKS_SCREENSHOT_STORED_MASKED"
backlinks_window_stored_digest=$(convert "$BACKLINKS_SCREENSHOT_STORED_MASKED" rgba:- | md5sum | awk '{print $1}')
if [ "$backlinks_window_digest" = "$backlinks_window_stored_digest" ]; then
    log "Masked \"Backlinks\" window screenshot matches the stored reference."
else
    error "Masked \"Backlinks\" window screenshot does not match the stored reference."
fi

log "Acquiring window geometry for window $backlinks_window_id..."
geom=$(xdotool getwindowgeometry --shell "$backlinks_window_id")
eval "$geom"
log "\"Backlinks\" window geometry: X=$X Y=$Y WIDTH=$WIDTH HEIGHT=$HEIGHT."

# Close the "Backlinks" window.
log "Clicking the \"Close\" button in the \"Backlinks\" window..."
close_x=$((X + WIDTH - 20))
close_y=$((Y + HEIGHT - 20))
run_and_time xdotool mousemove --sync "$close_x" "$close_y" click 1

log "Waiting up to 10 seconds for the \"Backlinks\" window to close..."
closed=false
for i in {1..100}; do
    if ! xwininfo -id "$backlinks_window_id" >/dev/null 2>&1; then
        closed=true
        break
    fi
    sleep 0.1
done
if $closed; then
    log "\"Backlinks\" window closed successfully."
else
    error "\"Backlinks\" window failed to close."
    exit 1
fi

# Now close the main window.
log "Clicking the \"Close\" button in the main window..."
main_close_x=$((main_X + main_WIDTH - 20))
main_close_y=$((main_Y + main_HEIGHT - 20))
# We do not use --sync here, because that causes the command to take very long
# (seconds) to complete, allegedly due to --sync mode having to wait for the
# whole application tear-down to complete when closing the last remaining
# window.
run_and_time xdotool mousemove "$main_close_x" "$main_close_y" click 1

log "Waiting up to 10 seconds for the main window to close..."
closed=false
for i in {1..100}; do
    if ! xwininfo -id "$main_window_id" >/dev/null 2>&1; then
        closed=true
        break
    fi
    sleep 0.1
done
if $closed; then
    log "Main window closed successfully."
else
    error "Main window failed to close."
fi

exit 0
