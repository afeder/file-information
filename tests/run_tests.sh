#!/bin/sh
set -e

cargo test --verbose
"$(dirname "$0")/graphical/run_tests.sh"
