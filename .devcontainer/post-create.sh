#!/usr/bin/env bash
# post-create.sh — Runs once when the devcontainer is first created.
# Verify the tooling the containers submodule provides, install the one thing
# it doesn't (clang, needed by our mold linker config), and warm the cargo cache
# so subsequent starts are fast.

set -euo pipefail

echo "==> Verifying lefthook..."
# lefthook is preinstalled in the devcontainer via the containers dev-tools feature.
command -v lefthook >/dev/null || {
  echo "ERROR: lefthook not on PATH (expected from the containers dev-tools feature)"
  exit 1
}

echo "==> Verifying cargo (Rust toolchain)..."
command -v cargo >/dev/null || {
  echo "ERROR: cargo not on PATH (expected from the containers rust-dev feature)"
  exit 1
}

# .cargo/config.toml sets linker = "clang" + -fuse-ld=mold for Linux. mold
# ships with the containers rust-dev feature; clang does not (only libclang-dev
# for bindgen), so install it explicitly. Without this, `cargo test` and
# rustdoc fail to link.
if ! command -v clang >/dev/null; then
  echo "==> Installing clang (needed by .cargo/config.toml linker setting)..."
  sudo apt-get update -qq
  sudo apt-get install -y --no-install-recommends clang
fi

echo "==> Warming cargo cache..."
cargo fetch --manifest-path /workspace/shannon/Cargo.toml

echo "==> Post-create setup complete."
