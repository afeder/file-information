#!/bin/sh
set -e

cargo test
"$(dirname "$0")/graphical/run_tests.sh"
