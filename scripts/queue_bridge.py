import redis
import json
import time
import os

# Connect to Redis (assuming localhost forwarding or reachable)
# In dev environment usually localhost:6379 works if port forwarded, 
# or use service name if running in pod.
# Script running on HOST needs port forward.
redis_url = os.getenv("REDIS_URL", "redis://localhost:6379")
r = redis.from_url(redis_url)

LEGACY_QUEUES = [
    "optimus:queue:python",
    "optimus:queue:java",
    "optimus:queue:rust"
]

UNIFIED_QUEUE = "optimus:queue:jobs"

print(f"Starting Queue Bridge...")
print(f"Monitoring {LEGACY_QUEUES} -> Forwarding to {UNIFIED_QUEUE}")

while True:
    # BLPOP is blocking, but we need to check multiple queues.
    # We use a short timeout loop.
    try:
        # pop from any legacy queue
        result = r.blpop(LEGACY_QUEUES, timeout=1)
        if result:
            queue_name, payload = result
            # queue_name is bytes, payload is bytes
            print(f"Forwarding job from {queue_name.decode()} to {UNIFIED_QUEUE}")
            r.rpush(UNIFIED_QUEUE, payload)
    except redis.ConnectionError:
        print("Redis connection error, retrying...")
        time.sleep(5)
    except Exception as e:
        print(f"Error: {e}")
        time.sleep(1)
