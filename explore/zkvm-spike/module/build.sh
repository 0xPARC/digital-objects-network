#!/usr/bin/env bash
# Builds the craft-basics module for both interpreters, plain and bloated, into out/.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p out
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C link-arg=-zstack-size=16384 -C link-arg=--initial-memory=65536"
export CARGO_TARGET_RISCV32IM_UNKNOWN_NONE_ELF_RUSTFLAGS="-C link-arg=-T$PWD/rv32.ld"
for variant in plain bloat evil; do
  features=""; suffix=""
  if [[ $variant != plain ]]; then features="--features $variant"; suffix="_$variant"; fi
  cargo build --release --lib --target wasm32-unknown-unknown $features
  cp target/wasm32-unknown-unknown/release/craft_basics_module.wasm "out/craft_basics$suffix.wasm"
  cargo build --release --bin craft-basics-rv32 --target riscv32im-unknown-none-elf $features
  cp target/riscv32im-unknown-none-elf/release/craft-basics-rv32 "out/craft_basics$suffix.rv32.elf"
done
ls -l out
