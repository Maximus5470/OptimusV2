#!/usr/bin/env python3
"""
Optimus Python Runner
Executes user code with test input in a sandboxed environment
"""
import os
import sys
import base64

def main():
    # Read base64-encoded source code and input from environment
    source_code_b64 = os.environ.get('SOURCE_CODE', '')
    test_input_b64 = os.environ.get('TEST_INPUT', '')
    
    if not source_code_b64:
        print("Error: SOURCE_CODE environment variable not set", file=sys.stderr)
        sys.exit(1)
    
    # Decode source code and input
    try:
        source_code = base64.b64decode(source_code_b64).decode('utf-8')
        test_input = base64.b64decode(test_input_b64).decode('utf-8') if test_input_b64 else ''
    except Exception as e:
        print(f"Error decoding input: {e}", file=sys.stderr)
        sys.exit(1)
    
    # Write source code to file
    with open('/code/main.py', 'w') as f:
        f.write(source_code)
    
    # Write test input to stdin
    # We'll use a technique to provide input via sys.stdin
    import io
    sys.stdin = io.StringIO(test_input)
    
    # Execute the user code
    try:
        with open('/code/main.py', 'r') as f:
            code = compile(f.read(), '/code/main.py', 'exec')
            exec(code, {'__name__': '__main__'})
    except Exception as e:
        print(f"{type(e).__name__}: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == '__main__':
    main()
