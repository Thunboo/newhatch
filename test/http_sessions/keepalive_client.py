#!/usr/bin/env python3
"""Send numbered GET/POST pairs on one verified TCP connection."""

import argparse
import http.client
import json
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("host", help="Fixture host, without http:// or a port")
    parser.add_argument("--port", type=int, default=18081)
    parser.add_argument("--count", type=int, default=5)
    parser.add_argument("--hold", type=float, default=20, help="Seconds to stay open after all replies")
    parser.add_argument("--pause-after", type=int, default=0, help="Request number before a 20-second idle pause")
    args = parser.parse_args()
    if args.count < 1 or args.hold < 0 or not 0 <= args.pause_after < args.count:
        parser.error("count must be positive, hold nonnegative, and pause-after less than count")

    conn = http.client.HTTPConnection(args.host, args.port, timeout=10)
    try:
        conn.connect()
        original_socket = conn.sock
        print(f"TCP local={original_socket.getsockname()} -> {args.host}:{args.port}", flush=True)
        connection_id = None
        for number in range(1, args.count + 1):
            if conn.sock is not original_socket:
                raise RuntimeError("TCP closed; refusing to reconnect for this test")
            method = "GET" if number % 2 else "POST"
            path = f"/test/{number}"
            body = "" if method == "GET" else f"request-{number}"
            conn.request(method, path, body=body.encode("utf-8"))
            response = conn.getresponse()
            data = json.loads(response.read())
            if response.status != 200 or conn.sock is not original_socket:
                raise RuntimeError("Unexpected status or closed TCP connection")
            if connection_id is None:
                connection_id = data["connection_id"]
            expected = {
                "connection_id": connection_id,
                "request": path,
                "method": method,
                "body": body,
                "reply": f"reply-{number}",
                "flag": "FLAG{KEEPALIVE_TEST_3}" if number == 3 else "",
            }
            if data != expected:
                raise RuntimeError(f"Unexpected response for {path}: {data!r}")
            print(
                f"{number}: {method} {path} -> 200 {data['reply']} "
                f"connection={connection_id}",
                flush=True,
            )
            if number == args.pause_after:
                print("Idle for 20 seconds, then continuing on the same TCP connection.", flush=True)
                time.sleep(20)
        print(
            f"OK: {args.count} matching pairs on one TCP connection. "
            f"Keeping it open for {args.hold:g}s; inspect newhatch now.",
            flush=True,
        )
        time.sleep(args.hold)
    finally:
        conn.close()


if __name__ == "__main__":
    main()
