#!/bin/bash
set -ex

cargo check
# Test build for the `thumbv6m-none-eabi` target, to make sure it compiles without `std`.
cargo build --no-default-features --target thumbv6m-none-eabi
cargo test
cargo fmt
cargo clippy
cargo doc --no-deps
