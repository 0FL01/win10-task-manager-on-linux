#!/bin/bash

set -e

cd "$(git rev-parse --show-toplevel)"

cargo fmt --all --check
cargo fmt --manifest-path subprojects/magpie/Cargo.toml --all --check
