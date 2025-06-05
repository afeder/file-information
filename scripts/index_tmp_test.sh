#!/bin/bash
set -e

# Use a test directory under the user's home so Tracker can index it
TEST_DIR="$HOME/test"
mkdir -p "$TEST_DIR"
if [ ! -f "$TEST_DIR/yourfile.txt" ]; then
    echo "This is a test" > "$TEST_DIR/yourfile.txt"
fi

# Define XDG paths if not already defined
export XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"

# Abort if configuration already exists and differs
config_file="$XDG_CONFIG_HOME/tracker3/miners/fs.cfg"
if [ -f "$config_file" ] && ! grep -Fxq "IndexRecursiveDirectories=$TEST_DIR" "$config_file"; then
    echo "Error: existing Tracker configuration at $config_file would be overwritten." >&2
    exit 1
fi

# Abort if Tracker daemon is running
if pgrep -f tracker-miner-fs >/dev/null 2>&1; then
    echo "Error: Tracker daemon is already running. Aborting." >&2
    exit 1
fi

# Start a D-Bus session if needed
if [ -z "$DBUS_SESSION_BUS_ADDRESS" ]; then
    addr=$(dbus-daemon --session --fork --print-address)
    export DBUS_SESSION_BUS_ADDRESS="$addr"
fi

# Configure Tracker to index $TEST_DIR
mkdir -p "$XDG_CONFIG_HOME/tracker3/miners"
cat <<EOT > "$XDG_CONFIG_HOME/tracker3/miners/fs.cfg"
[Indexing]
IndexRecursiveDirectories=$TEST_DIR
EOT

# Add directory to index and start Tracker3 daemon
tracker3 index --add --recursive "$TEST_DIR"
tracker3 daemon -s

# Wait for indexing to complete and display info
tracker3 status
tracker3 info "$TEST_DIR/yourfile.txt"