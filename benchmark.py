#!/usr/bin/env python3
"""
OptimusV2 Benchmark Test
Measures throughput, latency, and success rate for code execution API
"""

import requests
import time
import argparse
import statistics
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from typing import List, Optional

@dataclass
class BenchmarkConfig:
    api_url: str = "http://localhost:80"
    language: str = "python"
    concurrency: int = 20
    requests_count: int = 50
    timeout: int = 30

@dataclass
class BenchmarkResult:
    success: bool
    latency_ms: float
    status_code: Optional[int] = None
    error: Optional[str] = None

# Job templates for each language
JOB_TEMPLATES = {
    "python": {
        "language": "python",
        "source_code": """def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)

result = fibonacci(10)
print(f"Fibonacci(10) = {result}")
""",
        "test_cases": [{"id": 1, "input": "", "expected_output": "Fibonacci(10) = 55\n"}],
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
        "test_cases": [{"id": 1, "input": "", "expected_output": "Factorial(5) = 120\n"}],
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
        "test_cases": [{"id": 1, "input": "", "expected_output": "Sum = 15\n"}],
        "timeout_ms": 15000
    }
}

def send_request(config: BenchmarkConfig, request_id: int) -> BenchmarkResult:
    """Send a single request and measure latency"""
    job_data = JOB_TEMPLATES.get(config.language)
    if not job_data:
        return BenchmarkResult(success=False, latency_ms=0, error=f"Unknown language: {config.language}")
    
    try:
        start_time = time.perf_counter()
        response = requests.post(
            f"{config.api_url}/execute",
            json=job_data,
            timeout=config.timeout
        )
        latency_ms = (time.perf_counter() - start_time) * 1000
        
        # Consider 200-299 as success, also accept job_id response (async queuing)
        success = 200 <= response.status_code < 300
        
        return BenchmarkResult(
            success=success,
            latency_ms=latency_ms,
            status_code=response.status_code
        )
    except requests.exceptions.Timeout:
        return BenchmarkResult(success=False, latency_ms=config.timeout * 1000, error="Timeout")
    except requests.exceptions.ConnectionError as e:
        return BenchmarkResult(success=False, latency_ms=0, error=f"Connection error: {str(e)[:50]}")
    except Exception as e:
        return BenchmarkResult(success=False, latency_ms=0, error=str(e)[:50])

def calculate_percentile(latencies: List[float], percentile: int) -> float:
    """Calculate percentile from sorted latencies"""
    if not latencies:
        return 0.0
    sorted_latencies = sorted(latencies)
    index = int(len(sorted_latencies) * percentile / 100)
    index = min(index, len(sorted_latencies) - 1)
    return sorted_latencies[index]

def run_benchmark(config: BenchmarkConfig) -> None:
    """Run the benchmark with given configuration"""
    print(f"Starting {config.language.capitalize()} benchmark against {config.api_url}/execute")
    print(f"  concurrency: {config.concurrency}")
    print(f"  requests: {config.requests_count}")
    print()
    
    results: List[BenchmarkResult] = []
    start_time = time.perf_counter()
    
    with ThreadPoolExecutor(max_workers=config.concurrency) as executor:
        futures = {
            executor.submit(send_request, config, i): i 
            for i in range(config.requests_count)
        }
        
        for future in as_completed(futures):
            result = future.result()
            results.append(result)
    
    total_time = time.perf_counter() - start_time
    
    # Calculate statistics
    successful = [r for r in results if r.success]
    failed = [r for r in results if not r.success]
    latencies = [r.latency_ms for r in successful]
    
    success_count = len(successful)
    total_count = len(results)
    success_rate = (success_count / total_count * 100) if total_count > 0 else 0
    throughput = total_count / total_time if total_time > 0 else 0
    
    # Print results
    print("Results:")
    print(f"    Total Time: {total_time:.2f}s")
    print(f"    Throughput: {throughput:.2f} req/s")
    print(f"    Success Rate: {success_count}/{total_count} ({success_rate:.1f}%)")
    
    if latencies:
        avg_latency = statistics.mean(latencies)
        min_latency = min(latencies)
        max_latency = max(latencies)
        p50_latency = calculate_percentile(latencies, 50)
        p95_latency = calculate_percentile(latencies, 95)
        
        print(f"    Avg Latency: {avg_latency:.2f}ms")
        print(f"    Min Latency: {min_latency:.2f}ms")
        print(f"    Max Latency: {max_latency:.2f}ms")
        print(f"    P50 Latency: {p50_latency:.2f}ms")
        print(f"    P95 Latency: {p95_latency:.2f}ms")
    else:
        print("    No successful requests to calculate latency statistics")
    
    # Print errors if any
    if failed:
        print()
        print(f"Errors ({len(failed)} failures):")
        error_counts = {}
        for r in failed:
            error = r.error or f"HTTP {r.status_code}"
            error_counts[error] = error_counts.get(error, 0) + 1
        for error, count in list(error_counts.items())[:5]:
            print(f"    {error}: {count}")

def main():
    parser = argparse.ArgumentParser(description="OptimusV2 Benchmark Test")
    parser.add_argument("--url", default="http://localhost:80", help="API base URL")
    parser.add_argument("--language", "-l", default="python", choices=["python", "java", "rust"], help="Language to test")
    parser.add_argument("--concurrency", "-c", type=int, default=20, help="Number of concurrent requests")
    parser.add_argument("--requests", "-n", type=int, default=50, help="Total number of requests")
    parser.add_argument("--timeout", "-t", type=int, default=30, help="Request timeout in seconds")
    
    args = parser.parse_args()
    
    config = BenchmarkConfig(
        api_url=args.url,
        language=args.language,
        concurrency=args.concurrency,
        requests_count=args.requests,
        timeout=args.timeout
    )
    
    run_benchmark(config)

if __name__ == "__main__":
    main()
