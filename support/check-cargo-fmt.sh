#!/bin/bash

set -e

cd "$(git rev-parse --show-toplevel)"

cargo fmt --all --check
