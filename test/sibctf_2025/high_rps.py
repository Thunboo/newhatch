import argparse
import asyncio
import time
from collections import Counter

import aiohttp


DEFAULT_URL = (
    "http://100.97.65.117:18080/"
    "flag?player=team01&payload=demo-exploit"
)

# Sending in batches is much more reliable than trying to sleep 333 µs
# between requests at 3000 RPS.
BATCH_INTERVAL = 0.010  # 10 ms

# Prevent an overloaded destination from causing unlimited task buildup.
MAX_IN_FLIGHT = 10_000


started = 0
completed = 0
errors = 0
in_flight = 0

status_codes = Counter()
error_types = Counter()


async def do_request(session):
    global completed, errors, in_flight

    try:
        async with session.get(DEFAULT_URL) as response:
            await response.read()

            completed += 1
            status_codes[response.status] += 1

    except Exception as exc:
        errors += 1
        error_types[type(exc).__name__] += 1

    finally:
        in_flight -= 1


async def reporter():
    global started, completed, errors

    previous_started = started
    previous_completed = completed
    previous_errors = errors
    previous_time = time.perf_counter()

    while True:
        await asyncio.sleep(1)

        now = time.perf_counter()
        elapsed = now - previous_time

        current_started = started
        current_completed = completed
        current_errors = errors

        start_rps = (current_started - previous_started) / elapsed
        complete_rps = (current_completed - previous_completed) / elapsed
        error_rps = (current_errors - previous_errors) / elapsed

        statuses = " ".join(
            f"{code}:{count}"
            for code, count in sorted(status_codes.items())
        )

        error_summary = " ".join(
            f"{name}:{count}"
            for name, count in error_types.most_common(5)
        )

        print(
            f"interval={elapsed:.3f}s "
            f"started={start_rps:.0f}/s "
            f"completed={complete_rps:.0f}/s "
            f"errors={error_rps:.0f}/s "
            f"in_flight={in_flight}"
        )

        if statuses:
            print(f"  statuses(total): {statuses}")

        if error_summary:
            print(f"  errors(total):   {error_summary}")

        previous_started = current_started
        previous_completed = current_completed
        previous_errors = current_errors
        previous_time = now


async def load_generator(rps):
    global started, in_flight

    timeout = aiohttp.ClientTimeout(
        total=5,
        connect=2,
        sock_connect=2,
        sock_read=3,
    )

    connector = aiohttp.TCPConnector(
        limit=MAX_IN_FLIGHT,
        ttl_dns_cache=300,
        enable_cleanup_closed=True,
    )

    async with aiohttp.ClientSession(
        connector=connector,
        timeout=timeout,
    ) as session:

        asyncio.create_task(reporter())

        # Fractional accumulator allows rates that don't divide cleanly
        # into 10 ms batches.
        requests_per_batch = rps * BATCH_INTERVAL
        accumulator = 0.0

        next_batch = time.perf_counter()

        while True:
            now = time.perf_counter()

            if now < next_batch:
                await asyncio.sleep(next_batch - now)

            # If scheduling fell very far behind, don't generate a giant
            # catch-up burst.
            now = time.perf_counter()

            if now - next_batch > 0.100:
                next_batch = now

            accumulator += requests_per_batch
            batch_size = int(accumulator)
            accumulator -= batch_size

            available = MAX_IN_FLIGHT - in_flight

            if available > 0:
                count = min(batch_size, available)

                for _ in range(count):
                    started += 1
                    in_flight += 1
                    asyncio.create_task(do_request(session))

            next_batch += BATCH_INTERVAL


def parse_args():
    parser = argparse.ArgumentParser(
        description="Fixed-rate HTTP request generator"
    )

    parser.add_argument(
        "rps",
        type=float,
        help="Target requests per second, e.g. 3000",
    )

    return parser.parse_args()
/home #