#!/usr/bin/env python3
"""Small HTTP/1.1 echo fixture for live atomic-session capture checks."""

import argparse
import json
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def setup(self):
        super().setup()
        self.connection.settimeout(60)
        self.connection_id = uuid.uuid4().hex[:12]

    def reply(self):
        length = int(self.headers.get("Content-Length", "0"))
        request_body = self.rfile.read(length).decode("utf-8")
        request_id = self.path.rsplit("/", 1)[-1]
        body = json.dumps(
            {
                "connection_id": self.connection_id,
                "request": self.path,
                "method": self.command,
                "body": request_body,
                "reply": "reply-" + request_id,
                "flag": "FLAG{KEEPALIVE_TEST_3}" if request_id == "3" else "",
            }
        ).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        print(
            f"connection={self.connection_id} peer={self.client_address} "
            f"{self.command} {self.path}",
            flush=True,
        )

    do_GET = reply
    do_POST = reply


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=18081)
    args = parser.parse_args()
    with ThreadingHTTPServer((args.bind, args.port), Handler) as server:
        print(f"Listening on {args.bind}:{server.server_port}; stop with Ctrl-C.", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
