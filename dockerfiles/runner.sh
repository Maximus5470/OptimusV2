#!/bin/bash
# Universal Optimus Code Runner
# This script detects the language and executes code appropriately
# Supports: Python, Java, Rust, C++, Go, Node.js, and more
#
# CRITICAL: All execution commands MUST explicitly propagate exit codes
# to ensure runtime errors are detected by the Docker engine
#
# EXECUTION MODES (Phase 2: Compile-once support):
# - compile_and_run (default): Legacy mode - compile and execute in one step
# - compile: Only compile the code, don't execute
# - execute: Only execute pre-compiled code (assumes compilation already done)

set -e
set -o pipefail

# Read environment variables
SOURCE_CODE_B64="${SOURCE_CODE:-}"
TEST_INPUT_B64="${TEST_INPUT:-}"
LANGUAGE="${LANGUAGE:-}"
EXECUTION_MODE="${EXECUTION_MODE:-compile_and_run}"

# Validate required variables
if [ -z "$LANGUAGE" ]; then
    echo "Error: LANGUAGE environment variable not set" >&2
    exit 1
fi

# For compile_and_run and compile modes, SOURCE_CODE is required
if [ "$EXECUTION_MODE" = "compile_and_run" ] || [ "$EXECUTION_MODE" = "compile" ]; then
    if [ -z "$SOURCE_CODE_B64" ]; then
        echo "Error: SOURCE_CODE environment variable not set" >&2
        exit 1
    fi
fi

# Decode source code (if provided)
if [ -n "$SOURCE_CODE_B64" ]; then
    SOURCE_CODE=$(echo "$SOURCE_CODE_B64" | base64 -d)
fi

# Decode test input (if provided)
TEST_INPUT=$(echo "$TEST_INPUT_B64" | base64 -d 2>/dev/null || echo "")

# Create code directory if it doesn't exist
mkdir -p /code
cd /code

# Execute based on mode and language
case "$EXECUTION_MODE" in
    compile)
        # COMPILE-ONLY MODE: Just compile, don't execute
        case "$LANGUAGE" in
            python)
                # Write Python code
                echo "$SOURCE_CODE" > /code/main.py
                # Python: syntax check only
                python3 -m py_compile /code/main.py
                exit $?
                ;;
            
            java)
                # Write Java code
                echo "$SOURCE_CODE" > /code/Main.java
                # Unset JAVA_TOOL_OPTIONS to suppress informational messages
                unset JAVA_TOOL_OPTIONS
                # Compile Java code
                javac /code/Main.java 2>&1
                exit $?
                ;;
            
            rust)
                # Write Rust code
                echo "$SOURCE_CODE" > /code/main.rs
                # Compile Rust code
                rustc /code/main.rs -o /code/main 2>&1
                exit $?
                ;;
            
            cpp|c++)
                # Write C++ code
                echo "$SOURCE_CODE" > /code/main.cpp
                # Compile C++ code
                g++ -std=c++17 -O2 /code/main.cpp -o /code/main 2>&1
                exit $?
                ;;
            
            c)
                # Write C code
                echo "$SOURCE_CODE" > /code/main.c
                # Compile C code
                gcc -std=c11 -O2 /code/main.c -o /code/main 2>&1
                exit $?
                ;;
            
            go)
                # Write Go code
                echo "$SOURCE_CODE" > /code/main.go
                # Compile Go code (not run)
                go build -o /code/main /code/main.go 2>&1
                exit $?
                ;;
            
            typescript|ts)
                # Write TypeScript code
                echo "$SOURCE_CODE" > /code/main.ts
                # Compile TypeScript
                tsc /code/main.ts 2>&1
                exit $?
                ;;
            
            kotlin)
                # Write Kotlin code
                echo "$SOURCE_CODE" > /code/Main.kt
                # Compile Kotlin code
                kotlinc /code/Main.kt -include-runtime -d /code/main.jar 2>&1
                exit $?
                ;;
            
            swift)
                # Write Swift code
                echo "$SOURCE_CODE" > /code/main.swift
                # Compile Swift code
                swiftc /code/main.swift -o /code/main 2>&1
                exit $?
                ;;
            
            csharp|cs)
                # Write C# code
                echo "$SOURCE_CODE" > /code/Main.cs
                # Compile C# code
                csc /code/Main.cs /out:/code/main.exe 2>&1
                exit $?
                ;;
            
            # Interpreted languages don't need compilation
            javascript|node|nodejs|ruby|php|scala)
                echo "$SOURCE_CODE" > /code/main.$LANGUAGE
                echo "Interpreted language - no compilation needed"
                exit 0
                ;;
            
            *)
                echo "Error: Unsupported language '$LANGUAGE' for compile mode" >&2
                exit 1
                ;;
        esac
        ;;
    
    execute)
        # EXECUTE-ONLY MODE: Run pre-compiled code
        case "$LANGUAGE" in
            python)
                # Execute Python code (assumes main.py exists)
                echo "$TEST_INPUT" | python3 -u /code/main.py
                exit $?
                ;;
            
            java)
                # Execute compiled Java code
                unset JAVA_TOOL_OPTIONS
                echo "$TEST_INPUT" | java -cp /code Main
                exit $?
                ;;
            
            rust)
                # Execute compiled Rust binary
                echo "$TEST_INPUT" | /code/main
                exit $?
                ;;
            
            cpp|c++)
                # Execute compiled C++ binary
                echo "$TEST_INPUT" | /code/main
                exit $?
                ;;
            
            c)
                # Execute compiled C binary
                echo "$TEST_INPUT" | /code/main
                exit $?
                ;;
            
            go)
                # Execute compiled Go binary
                echo "$TEST_INPUT" | /code/main
                exit $?
                ;;
            
            typescript|ts)
                # Execute compiled JavaScript
                echo "$TEST_INPUT" | node /code/main.js
                exit $?
                ;;
            
            kotlin)
                # Execute compiled Kotlin JAR
                echo "$TEST_INPUT" | java -jar /code/main.jar
                exit $?
                ;;
            
            swift)
                # Execute compiled Swift binary
                echo "$TEST_INPUT" | /code/main
                exit $?
                ;;
            
            csharp|cs)
                # Execute compiled C# binary
                echo "$TEST_INPUT" | mono /code/main.exe
                exit $?
                ;;
            
            javascript|node|nodejs)
                # Execute JavaScript
                echo "$TEST_INPUT" | node /code/main.js
                exit $?
                ;;
            
            ruby)
                # Execute Ruby
                echo "$TEST_INPUT" | ruby /code/main.rb
                exit $?
                ;;
            
            php)
                # Execute PHP
                echo "$TEST_INPUT" | php /code/main.php
                exit $?
                ;;
            
            scala)
                # Execute Scala
                echo "$TEST_INPUT" | scala /code/Main.scala
                exit $?
                ;;
            
            *)
                echo "Error: Unsupported language '$LANGUAGE' for execute mode" >&2
                exit 1
                ;;
        esac
        ;;
    
    compile_and_run)
        # LEGACY MODE: Compile and execute in one step (original behavior)
        case "$LANGUAGE" in
            python)
        # Write Python code
        echo "$SOURCE_CODE" > /code/main.py
        
        # Execute Python code with test input
        echo "$TEST_INPUT" | python3 -u /code/main.py
        # CRITICAL: Propagate exit code to Docker
        exit $?
        ;;
        
    java)
        # Write Java code
        echo "$SOURCE_CODE" > /code/Main.java
        
        # Unset JAVA_TOOL_OPTIONS to suppress the informational message
        unset JAVA_TOOL_OPTIONS
        
        # Compile Java code
        javac /code/Main.java 2>&1
        
        if [ $? -ne 0 ]; then
            echo "Compilation failed" >&2
            exit 1
        fi
        
        # Execute Java code with test input
        echo "$TEST_INPUT" | java -cp /code Main
        # CRITICAL: Propagate exit code to Docker
        exit $?
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
        # CRITICAL: Propagate exit code to Docker
        exit $?
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
        # CRITICAL: Propagate exit code to Docker
        exit $?
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
        # CRITICAL: Propagate exit code to Docker
        exit $?
        ;;
        
    go)
        # Write Go code
        echo "$SOURCE_CODE" > /code/main.go
        
        # Execute Go code with test input (compile and run)
        echo "$TEST_INPUT" | go run /code/main.go
        # CRITICAL: Propagate exit code to Docker
        exit $?
        ;;
        
    javascript|node|nodejs)
        # Write JavaScript code
        echo "$SOURCE_CODE" > /code/main.js
        
        # Execute Node.js code with test input
        echo "$TEST_INPUT" | node /code/main.js
        # CRITICAL: Propagate exit code to Docker
        exit $?
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
        # CRITICAL: Propagate exit code to Docker
        exit $?
        ;;
        
    ruby)
        # Write Ruby code
        echo "$SOURCE_CODE" > /code/main.rb
        
        # Execute Ruby code with test input
        echo "$TEST_INPUT" | ruby /code/main.rb
        # CRITICAL: Propagate exit code to Docker
        exit $?
        ;;
        
    php)
        # Write PHP code
        echo "$SOURCE_CODE" > /code/main.php
        
        # Execute PHP code with test input
        echo "$TEST_INPUT" | php /code/main.php
        # CRITICAL: Propagate exit code to Docker
        exit $?
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
        # CRITICAL: Propagate exit code to Docker
        exit $?
        ;;
        
    scala)
        # Write Scala code
        echo "$SOURCE_CODE" > /code/Main.scala
        
        # Compile and execute Scala code with test input
        echo "$TEST_INPUT" | scala /code/Main.scala
        # CRITICAL: Propagate exit code to Docker
        exit $?
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
        # CRITICAL: Propagate exit code to Docker
        exit $?
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
        # CRITICAL: Propagate exit code to Docker
        exit $?
        ;;
        
    *)
        echo "Error: Unsupported language '$LANGUAGE'" >&2
        echo "Supported languages: python, java, rust, cpp, c, go, javascript, typescript, ruby, php, kotlin, scala, csharp, swift" >&2
        exit 1
        ;;
esac
        ;;
    
    *)
        echo "Error: Invalid EXECUTION_MODE '$EXECUTION_MODE'" >&2
        echo "Valid modes: compile, execute, compile_and_run" >&2
        exit 1
        ;;
esac