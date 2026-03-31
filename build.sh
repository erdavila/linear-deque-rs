#!/bin/bash
set -ex

cargo test
cargo fmt
cargo clippy
