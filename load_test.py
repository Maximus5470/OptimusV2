#!/usr/bin/env python3
"""
Load testing script for OptimusV2 autoscaling validation
Sends 500 concurrent job requests to test KEDA autoscaling behavior
Distribution: Python (50%), Java (40%), Rust (10%)
"""

import requests
import time
import json
from concurrent.futures import ThreadPoolExecutor, as_completed
from collections import defaultdict
import sys

API_URL = "http://localhost:80"
TOTAL_REQUESTS = 500

# Job templates
JOBS = {
    "python": {
        "language": "python",
        "source_code": """def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)

result = fibonacci(10)
print(f"Fibonacci(10) = {result}")
""",
        "test_cases": [
            {
                "id": 1,
                "input": "",
                "expected_output": "Fibonacci(10) = 55\n"
            }
        ],
        "timeout_ms": 10000
    },
    "java": {
        "language": "java",
        "source_code": """public class Solution {
    public static int factorial(int n) {
        if (n <= 1) return 1;
        return n * factorial(n - 1);
    }
    
    public static void main(String[] args) {
        int result = factorial(5);
        System.out.println("Factorial(5) = " + result);
    }
}
""",
        "test_cases": [
            {
                "id": 1,
                "input": "",
                "expected_output": "Factorial(5) = 120\n"
            }
        ],
        "timeout_ms": 15000
    },
    "rust": {
        "language": "rust",
        "source_code": """fn sum_array(arr: &[i32]) -> i32 {
    arr.iter().sum()
}

fn main() {
    let numbers = vec![1, 2, 3, 4, 5];
    let result = sum_array(&numbers);
    println!("Sum = {}", result);
}
""",
        "test_cases": [
            {
                "id": 1,
                "input": "",
                "expected_output": "Sum = 15\n"
            }
        ],
        "timeout_ms": 15000
    }
}

def send_request(request_id, language):
    """Send a single job request"""
    job_data = JOBS[language]
    
    try:
        start_time = time.time()
        response = requests.post(
            f"{API_URL}/execute",
            json=job_data,
            timeout=30
        )
        latency = int((time.time() - start_time) * 1000)
        
        if response.status_code == 200:
            return {
                'id': request_id,
                'language': language,
                'success': True,
                'latency': latency,
                'status': response.status_code
            }
        else:
            return {
                'id': request_id,
                'language': language,
                'success': False,
                'latency': latency,
                'status': response.status_code,
                'error': response.text[:100]
            }
    except Exception as e:
        return {
            'id': request_id,
            'language': language,
            'success': False,
            'latency': 0,
            'error': str(e)[:100]
        }

def main():
    print(f"[*] OptimusV2 Load Test Starting...")
    print(f"[*] Total Requests: {TOTAL_REQUESTS}")
    print(f"[*] API URL: {API_URL}")
    
    # Distribution: Python 50%, Java 40%, Rust 10%
    python_count = int(TOTAL_REQUESTS * 0.50)
    java_count = int(TOTAL_REQUESTS * 0.40)
    rust_count = TOTAL_REQUESTS - python_count - java_count
    
    print(f"[*] Distribution: Python={python_count}, Java={java_count}, Rust={rust_count}")
    
    # Build request list
    requests_list = []
    req_id = 1
    
    for _ in range(python_count):
        requests_list.append((req_id, "python"))
        req_id += 1
    
    for _ in range(java_count):
        requests_list.append((req_id, "java"))
        req_id += 1
    
    for _ in range(rust_count):
        requests_list.append((req_id, "rust"))
        req_id += 1
    
    print(f"[*] Starting load test at {time.strftime('%H:%M:%S')}")
    print(f"[*] Monitor scaling with: kubectl get pods -n optimus -w")
    print(f"[*] Check queue lengths: kubectl exec -n optimus redis-xxx -- redis-cli LLEN optimus:queue:python")
    print()
    
    # Statistics
    stats = defaultdict(int)
    latencies = []
    start_time = time.time()
    
    # Send requests with high concurrency
    max_workers = 50
    with ThreadPoolExecutor(max_workers=max_workers) as executor:
        futures = {executor.submit(send_request, req_id, lang): (req_id, lang) for req_id, lang in requests_list}
        
        completed = 0
        for future in as_completed(futures):
            result = future.result()
            completed += 1
            
            if result['success']:
                stats[f"{result['language']}_success"] += 1
                latencies.append(result['latency'])
                if completed % 10 == 0:
                    print(f"[+] Progress: {completed}/{TOTAL_REQUESTS} ({int(completed/TOTAL_REQUESTS*100)}%) - Last: {result['language']} in {result['latency']}ms")
            else:
                stats[f"{result['language']}_failure"] += 1
                error_msg = result.get('error', 'Unknown error')[:50]
                print(f"[X] Request {result['id']} ({result['language']}) failed: {error_msg}")
    
    end_time = time.time()
    duration = end_time - start_time
    
    # Print results
    print()
    print("=" * 60)
    print("Load Test Complete!")
    print("=" * 60)
    print(f"Total Requests: {TOTAL_REQUESTS}")
    
    total_success = sum(v for k, v in stats.items() if 'success' in k)
    total_failure = sum(v for k, v in stats.items() if 'failure' in k)
    
    print(f"Successful: {total_success}")
    print(f"Failed: {total_failure}")
    print(f"Duration: {duration:.2f}s")
    print(f"Requests/sec: {TOTAL_REQUESTS/duration:.2f}")
    
    if latencies:
        avg_latency = sum(latencies) / len(latencies)
        print(f"Avg Latency: {avg_latency:.0f}ms")
        print(f"Min Latency: {min(latencies)}ms")
        print(f"Max Latency: {max(latencies)}ms")
    
    print()
    print("Per-Language Stats:")
    print(f"  Python: {stats['python_success']} success, {stats['python_failure']} failed")
    print(f"  Java:   {stats['java_success']} success, {stats['java_failure']} failed")
    print(f"  Rust:   {stats['rust_success']} success, {stats['rust_failure']} failed")
    
    print()
    print("[*] Check final pod count: kubectl get pods -n optimus")
    print("[*] View scaling events: kubectl get events -n optimus --sort-by='.lastTimestamp'")
    print("[*] Check HPA status: kubectl describe scaledobject -n optimus")

if __name__ == "__main__":
    main()
