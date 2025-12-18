#!/bin/bash
# Universal Optimus Code Runner
# This script detects the language and executes code appropriately
# Supports: Python, Java, Rust, C++, Go, Node.js, and more

set -e

# Read environment variables
SOURCE_CODE_B64="${SOURCE_CODE:-}"
TEST_INPUT_B64="${TEST_INPUT:-}"
LANGUAGE="${LANGUAGE:-}"

if [ -z "$SOURCE_CODE_B64" ]; then
    echo "Error: SOURCE_CODE environment variable not set" >&2
    exit 1
fi

if [ -z "$LANGUAGE" ]; then
    echo "Error: LANGUAGE environment variable not set" >&2
    exit 1
fi

# Decode source code and input
SOURCE_CODE=$(echo "$SOURCE_CODE_B64" | base64 -d)
TEST_INPUT=$(echo "$TEST_INPUT_B64" | base64 -d 2>/dev/null || echo "")

# Create code directory if it doesn't exist
mkdir -p /code
cd /code

# Execute based on language
case "$LANGUAGE" in
    python)
        # Write Python code
        echo "$SOURCE_CODE" > /code/main.py
        
        # Execute Python code with test input
        echo "$TEST_INPUT" | python3 -u /code/main.py
        ;;
        
    java)
        # Write Java code
        echo "$SOURCE_CODE" > /code/Main.java
        
        # Compile Java code
        javac /code/Main.java 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute Java code with test input
        echo "$TEST_INPUT" | java -cp /code Main
        ;;
        
    rust)
        # Write Rust code
        echo "$SOURCE_CODE" > /code/main.rs
        
        # Compile Rust code
        rustc /code/main.rs -o /code/main 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute Rust binary with test input
        echo "$TEST_INPUT" | /code/main
        ;;
        
    cpp|c++)
        # Write C++ code
        echo "$SOURCE_CODE" > /code/main.cpp
        
        # Compile C++ code
        g++ -std=c++17 -O2 /code/main.cpp -o /code/main 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute C++ binary with test input
        echo "$TEST_INPUT" | /code/main
        ;;
        
    c)
        # Write C code
        echo "$SOURCE_CODE" > /code/main.c
        
        # Compile C code
        gcc -std=c11 -O2 /code/main.c -o /code/main 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute C binary with test input
        echo "$TEST_INPUT" | /code/main
        ;;
        
    go)
        # Write Go code
        echo "$SOURCE_CODE" > /code/main.go
        
        # Execute Go code with test input (compile and run)
        echo "$TEST_INPUT" | go run /code/main.go
        ;;
        
    javascript|node|nodejs)
        # Write JavaScript code
        echo "$SOURCE_CODE" > /code/main.js
        
        # Execute Node.js code with test input
        echo "$TEST_INPUT" | node /code/main.js
        ;;
        
    typescript|ts)
        # Write TypeScript code
        echo "$SOURCE_CODE" > /code/main.ts
        
        # Compile TypeScript to JavaScript
        tsc /code/main.ts 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute compiled JavaScript with test input
        echo "$TEST_INPUT" | node /code/main.js
        ;;
        
    ruby)
        # Write Ruby code
        echo "$SOURCE_CODE" > /code/main.rb
        
        # Execute Ruby code with test input
        echo "$TEST_INPUT" | ruby /code/main.rb
        ;;
        
    php)
        # Write PHP code
        echo "$SOURCE_CODE" > /code/main.php
        
        # Execute PHP code with test input
        echo "$TEST_INPUT" | php /code/main.php
        ;;
        
    kotlin)
        # Write Kotlin code
        echo "$SOURCE_CODE" > /code/Main.kt
        
        # Compile Kotlin code
        kotlinc /code/Main.kt -include-runtime -d /code/main.jar 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute Kotlin JAR with test input
        echo "$TEST_INPUT" | java -jar /code/main.jar
        ;;
        
    scala)
        # Write Scala code
        echo "$SOURCE_CODE" > /code/Main.scala
        
        # Compile and execute Scala code with test input
        echo "$TEST_INPUT" | scala /code/Main.scala
        ;;
        
    csharp|cs)
        # Write C# code
        echo "$SOURCE_CODE" > /code/Main.cs
        
        # Compile C# code
        csc /code/Main.cs /out:/code/main.exe 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute C# binary with test input
        echo "$TEST_INPUT" | mono /code/main.exe
        ;;
        
    swift)
        # Write Swift code
        echo "$SOURCE_CODE" > /code/main.swift
        
        # Compile Swift code
        swiftc /code/main.swift -o /code/main 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute Swift binary with test input
        echo "$TEST_INPUT" | /code/main
        ;;
        
    *)
        echo "Error: Unsupported language '$LANGUAGE'" >&2
        echo "Supported languages: python, java, rust, cpp, c, go, javascript, typescript, ruby, php, kotlin, scala, csharp, swift" >&2
        exit 1
        ;;
esac
