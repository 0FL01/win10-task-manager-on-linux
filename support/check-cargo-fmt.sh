#!/bin/bash

set -e

cd "$(git rev-parse --show-toplevel)"

cargo fmt
DIFF=$(git diff)

if [ ! -z "$DIFF" ]; then
    echo "Source code not properly formatted, please run: \`cargo fmt\` in the root of the project."
    exit 1
fi
