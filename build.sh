#!/bin/bash
set -ex

cargo check
cargo check --no-default-features
cargo test
cargo fmt
cargo clippy
cargo doc --no-deps
