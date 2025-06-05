#!/bin/bash
set -e

# Use a test directory under the user's home so Tracker can index it
TEST_DIR="$HOME/test"
mkdir -p "$TEST_DIR"
if [ ! -f "$TEST_DIR/yourfile.txt" ]; then
    echo "This is a test" > "$TEST_DIR/yourfile.txt"
fi

# Add directory to index and start Tracker3 daemon
tracker3 index --add --recursive "$TEST_DIR"
tracker3 daemon -s

# Wait for indexing to complete and display info
tracker3 status
tracker3 info "$TEST_DIR/yourfile.txt"
