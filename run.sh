#!/usr/bin/env bash
set -euo pipefail

export DYLD_LIBRARY_PATH=/opt/homebrew/Cellar/instantclient-basic/19.8.0.0.0dbru/lib:${DYLD_LIBRARY_PATH:-}

cargo run --bin space_query "$@"
