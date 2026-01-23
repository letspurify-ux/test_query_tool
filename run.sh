#!/bin/bash
export DYLD_LIBRARY_PATH=/opt/homebrew/Cellar/instantclient-basic/19.8.0.0.0dbru/lib:$DYLD_LIBRARY_PATH
cargo run
