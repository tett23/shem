#!/bin/sh
# Builds the module for the browser, shem.wasm (docs/adr/0018).
#
#   build-wasm.sh <output file>
#
# The library is built for wasm32-unknown-unknown as a cdylib, without a
# change to Cargo.toml, and checked to start with the magic number of wasm.
set -eu

[ $# -eq 1 ] || { echo "usage: build-wasm.sh <output file>" >&2; exit 64; }
cargo rustc --release --locked --lib --target wasm32-unknown-unknown --crate-type cdylib
cp target/wasm32-unknown-unknown/release/shem.wasm "$1"
magic=$(od -An -tx1 -N4 "$1" | tr -d ' \n')
[ "$magic" = "0061736d" ] || { echo "build-wasm.sh: $1 is not a wasm module" >&2; exit 1; }
