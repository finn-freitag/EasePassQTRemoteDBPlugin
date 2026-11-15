#!/usr/bin/env bash
# Tests, builds, checks and packages the plugin (the same script runs in CI):
#   1. unit tests of the plugin logic (native)
#   2. build for wasm32-unknown-unknown and turn the module into a component
#   3. the component has exactly the types and permissions of expected.json and agrees
#      with manifest.json (tools/check_component.py, the reference check of the plugin spec)
#   4. package dist/<id>.epplugin (deterministic) and print its SHA-256
# Needs: cargo with the wasm32-unknown-unknown target, wasm-tools, python3 (`nix develop`).
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET=wasm32-unknown-unknown
rm -rf dist && mkdir -p dist
if [ "${SKIP_TESTS:-0}" != 1 ]; then
    echo "== 1. Tests"
    cargo test --locked -p easepass_url_database_core
fi
echo "== 2. Component"
cargo build --locked --release --target "$TARGET" -p easepass_url_database
wasm-tools component new "target/$TARGET/release/easepass_url_database.wasm" -o dist/plugin.wasm
wasm-tools validate dist/plugin.wasm
echo "== 3. Types and permissions"
python3 tools/check_component.py dist/plugin.wasm --expected expected.json --manifest manifest.json
echo "== 4. Package"
ICON=(); [ -f icon.png ] && ICON=(--icon icon.png)
python3 tools/package.py --manifest manifest.json --wasm dist/plugin.wasm "${ICON[@]}" --out-dir dist | tee dist/SHA256SUMS
