#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --locked check
(
  cd apps/desktop
  npm run format:check
  npm run check
  npm run lint
  npm test
  npm run build
  npm run tauri build -- --debug --bundles deb
)
desktop-file-validate target/debug/bundle/deb/*/data/usr/share/applications/*.desktop
