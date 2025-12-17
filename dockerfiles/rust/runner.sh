#!/bin/bash
# Optimus Rust Runner
# Executes Rust code with given input and captures output

set -e

# Read source code from environment (base64 encoded)
SOURCE_CODE_B64="${SOURCE_CODE:-}"
TEST_INPUT_B64="${TEST_INPUT:-}"

if [ -z "$SOURCE_CODE_B64" ]; then
    echo "Error: SOURCE_CODE environment variable not set" >&2
    exit 1
fi

# Decode source code and input
SOURCE_CODE=$(echo "$SOURCE_CODE_B64" | base64 -d)
TEST_INPUT=$(echo "$TEST_INPUT_B64" | base64 -d)

# Write source code to file
echo "$SOURCE_CODE" > /code/main.rs

# Compile the Rust code
rustc /code/main.rs -o /code/main 2>&1

if [ $? -ne 0 ]; then
    echo "Compilation failed" >&2
    exit 1
fi

# Execute with test input
echo "$TEST_INPUT" | /code/main
