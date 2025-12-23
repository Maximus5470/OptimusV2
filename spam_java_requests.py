import json
import time
import urllib.request
import urllib.error
from concurrent.futures import ThreadPoolExecutor, as_completed

API_URL = "http://localhost:3000/execute"
REQUEST_COUNT = 500
MAX_WORKERS = 50

JAVA_CODE = """
class Main {
    public static void main(String[] args) {
        java.util.Scanner sc = new java.util.Scanner(System.in);
        if (sc.hasNextLine()) {
            String input = sc.nextLine();
            System.out.println(input);
        }
        sc.close();
    }
}
"""

PAYLOAD = {
    "language": "java",
    "source_code": JAVA_CODE,
    "test_cases": [
        {
            "input": "Hello World",
            "expected_output": "Hello World",
            "weight": 10
        },
        {
            "input": "Test 123",
            "expected_output": "Test 123",
            "weight": 10
        }
    ],
    "timeout_ms": 10000
}

def send_request(req_id):
    headers = {
        "Content-Type": "application/json",
        "Idempotency-Key": f"load-test-py-{int(time.time())}-{req_id}"
    }
    data = json.dumps(PAYLOAD).encode('utf-8')
    req = urllib.request.Request(API_URL, data=data, headers=headers, method='POST')
    
    start_time = time.time()
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            resp_body = response.read()
            duration = (time.time() - start_time) * 1000
            return {
                "id": req_id,
                "success": True,
                "status": response.status,
                "duration": duration,
                "job_id": json.loads(resp_body).get("job_id")
            }
    except urllib.error.HTTPError as e:
        return {
            "id": req_id,
            "success": False,
            "status": e.code,
            "error": str(e),
            "duration": (time.time() - start_time) * 1000
        }
    except Exception as e:
        return {
            "id": req_id,
            "success": False,
            "status": 0,
            "error": str(e),
            "duration": (time.time() - start_time) * 1000
        }

def main():
    print(f"Starting load test: {REQUEST_COUNT} requests to {API_URL}")
    print(f"Concurrency: {MAX_WORKERS}")
    
    # Check health first
    try:
        with urllib.request.urlopen("http://localhost:3000/health", timeout=5) as response:
            if response.status == 200:
                print("API is reachable.")
            else:
                print(f"Warning: API returned status {response.status} on health check.")
    except Exception as e:
        print(f"Error: API not reachable at {API_URL}. {e}")
        return

    start_total = time.time()
    results = []
    
    with ThreadPoolExecutor(max_workers=MAX_WORKERS) as executor:
        futures = [executor.submit(send_request, i) for i in range(REQUEST_COUNT)]
        
        completed = 0
        for future in as_completed(futures):
            res = future.result()
            results.append(res)
            completed += 1
            if completed % 50 == 0:
                print(f"  [+] {completed}/{REQUEST_COUNT} completed")

    end_total = time.time()
    duration_total = end_total - start_total
    
    successes = [r for r in results if r['success']]
    failures = [r for r in results if not r['success']]
    
    print("\nLoad Test Complete")
    print(f"Total Duration: {duration_total:.2f}s")
    print(f"Requests/sec: {REQUEST_COUNT / duration_total:.2f}")
    print(f"Successful: {len(successes)}")
    print(f"Failed: {len(failures)}")
    
    if successes:
        avg_time = sum(r['duration'] for r in successes) / len(successes)
        print(f"Avg Response Time: {avg_time:.2f}ms")
        print(f"Sample Job IDs: {[r['job_id'] for r in successes[:5]]}")
    
    if failures:
        print("\nSample Failures:")
        for f in failures[:5]:
            print(f"  Req {f['id']}: {f.get('error')}")

if __name__ == "__main__":
    main()
