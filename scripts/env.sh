#!/bin/sh
# Optional helper for this workstation's isolated tools; normal installations use PATH.
task_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -d "$task_root/.tools/cargo" ]; then
  export CARGO_HOME="$task_root/.tools/cargo" RUSTUP_HOME="$task_root/.tools/rustup"
  export PATH="$task_root/.tools/cargo/bin:$PATH"
fi
runtime_node="/Users/t/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin"
runtime_bin="/Users/t/.cache/codex-runtimes/codex-primary-runtime/dependencies/bin/fallback"
if [ -d "$runtime_node" ]; then export PATH="$runtime_node:$runtime_bin:$PATH"; fi
exec "$@"
