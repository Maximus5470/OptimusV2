#!/usr/bin/env python3
"""
Load testing script for OptimusV2 autoscaling validation
Sends 500 concurrent job requests to test KEDA autoscaling behavior
Distribution: Python (50%), Java (40%), Rust (10%)
Includes automatic Redis Port Forwarding and Queue Bridging for Universal Worker compatibility.
"""

import requests
import time
import json
import threading
import subprocess
import os
import signal
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from collections import defaultdict

try:
    import redis
except ImportError:
    print("Error: 'redis' module not found. Please run 'pip install redis'")
    sys.exit(1)

API_URL = os.getenv("API_URL", "http://127.0.0.1:80")
TOTAL_REQUESTS = 100
REDIS_LOCAL_PORT = 6379

LEGACY_QUEUES = [
    "optimus:queue:python",
    "optimus:queue:java",
    "optimus:queue:rust"
]
UNIFIED_QUEUE = "optimus:queue:jobs"

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
        "source_code": """public class Main {
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

class RedisBridge(threading.Thread):
    def __init__(self, stop_event):
        super().__init__()
        self.stop_event = stop_event
        self.daemon = True
        self.redis_client = None

    def run(self):
        print("[Bridge] Connecting to Redis...")
        try:
            self.redis_client = redis.Redis(host='localhost', port=REDIS_LOCAL_PORT, db=0)
            self.redis_client.ping()
            print("[Bridge] Connected. Starting legacy queue monitoring...")
        except Exception as e:
            print(f"[Bridge] Error connecting to Redis: {e}")
            return

        while not self.stop_event.is_set():
            try:
                # Check legacy queues
                for queue in LEGACY_QUEUES:
                    # Pop from legacy queue (non-blocking for loop)
                    job = self.redis_client.rpop(queue)
                    if job:
                        # Push to unified queue
                        self.redis_client.lpush(UNIFIED_QUEUE, job)
                        # print(f"[Bridge] Forwarded job from {queue} to {UNIFIED_QUEUE}") # Verbose
            except Exception as e:
                if not self.stop_event.is_set():
                    print(f"[Bridge] Error: {e}")
                time.sleep(1)
            
            time.sleep(0.1) # Prevent tight loop

def start_port_forward():
    print("[*] Starting kubectl port-forward for Redis...")
    try:
        proc = subprocess.Popen(
            ["kubectl", "port-forward", "svc/redis", f"{REDIS_LOCAL_PORT}:6379", "-n", "optimus"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL
        )
        time.sleep(3) # Wait for connection
        return proc
    except Exception as e:
        print(f"[!] Warning: Failed to start port-forward: {e}")
        return None

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
        
        if response.status_code in [200, 201, 202]:
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

def wait_for_bridge_drain(redis_client, timeout=30):
    print(f"[*] Waiting for bridge to drain legacy queues (Timeout: {timeout}s)...")
    start = time.time()
    while time.time() - start < timeout:
        pending = 0
        try:
            for q in LEGACY_QUEUES:
                pending += redis_client.llen(q)
        except:
            pass
            
        if pending == 0:
            print("[*] Legacy queues drained.")
            return
        time.sleep(1)
    print(f"[!] Warning: Bridge timed out with {pending} jobs remaining.")

def main():
    print(f"[*] OptimusV2 Load Test Starting...")
    print(f"[*] Total Requests: {TOTAL_REQUESTS}")
    print(f"[*] API URL: {API_URL}")
    
    # Start Port Forward
    pf_process = start_port_forward()
    bridge_stop_event = threading.Event()
    bridge_thread = RedisBridge(bridge_stop_event)
    bridge_thread.start()

    try:
        # Give bridge a moment to connect
        time.sleep(2)
        # Distribution: Python 50%, Java 40%, Rust 10%
        python_count = int(TOTAL_REQUESTS * 0)
        java_count = int(TOTAL_REQUESTS * 1)
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
                    if completed % 50 == 0:
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
        print(f"[*] Worker Scaling Verification:")
        try:
           # Check redis queue length
           r = redis.Redis(host='localhost', port=REDIS_LOCAL_PORT, db=0)
           q_len = r.llen(UNIFIED_QUEUE)
           print(f"  Unified Queue Length: {q_len}")
        except:
           print("  Could not check queue length.")
           
        # Drain queues
        if bridge_thread.redis_client:
            wait_for_bridge_drain(bridge_thread.redis_client)

    finally:
        print("[*] Cleaning up...")
        bridge_stop_event.set()
        bridge_thread.join(timeout=2)
        if pf_process:
            pf_process.terminate()
            pf_process.wait()

if __name__ == "__main__":
    main()
