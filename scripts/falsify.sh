#!/usr/bin/env bash
# S4 falsifier: a test that calls nothing must report 0% line coverage, else the meter is lying.
set -euo pipefail
cargo +1.98.0 llvm-cov --ignore-filename-regex 'main\.rs' --json --output-path /dev/stdout \
  -- --ignored calls_nothing 2>/dev/null | grep -o '"lines":{[^}]*}' | tail -1 | tee /dev/stderr | grep -q '"percent":0'
echo "falsifier: 0% reported for a test that calls nothing: OK"
