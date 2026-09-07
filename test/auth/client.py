"""Executed inside clients with real, different socket source addresses."""
import http.client
import json
import socket
import sys

host, mode = sys.argv[1:3]
session = sys.stdin.read().strip() if mode == "denied" else ""


def request(method, path, body=None, cookie="", forged=False):
    connection = http.client.HTTPConnection(host, 8080, timeout=5)
    headers = {"Content-Type": "application/json", "Cookie": cookie}
    if forged:
        headers.update({"X-Forwarded-For": "192.168.1.100", "X-Real-IP": "127.0.0.1",
                        "Forwarded": "for=192.168.1.100", "CF-Connecting-IP": "192.168.1.100"})
    connection.request(method, path, json.dumps(body) if body is not None else None, headers)
    response = connection.getresponse()
    result = response.status, {name.lower(): value for name, value in response.getheaders()}, response.read()
    connection.close()
    return result


credentials = {"username": "team", "password": "test-only-password"}
assert request("GET", "/api/health")[::2] == (200, b'{"status":"ok"}')
try:
    with socket.create_connection((host, 3000), timeout=2):
        raise AssertionError("analyzer is externally reachable")
except (ConnectionRefusedError, TimeoutError):
    pass

if mode == "allowed":
    assert request("GET", "/")[0] == 200
    assert request("GET", "/logo.png")[0] == 200
    assert request("GET", "/api/sessions")[0] == 401
    assert request("POST", "/api/auth/login", {**credentials, "password": "wrong"})[0] == 401
    status, headers, _ = request("POST", "/api/auth/login", credentials)
    assert status == 200, status
    session = headers["set-cookie"].split(";")[0]
    assert request("GET", "/api/auth/me", cookie=session)[0] == 200
    assert request("GET", "/api/sessions", cookie=session)[0] == 200
    # Leave this session valid so other clients can test network rejection with it.
    print(session)
elif mode == "denied":
    for forged in [False, True]:
        for method, path, body in [("GET", "/", None), ("GET", "/logo.png", None),
                                   ("POST", "/api/auth/login", credentials), ("GET", "/api/auth/me", None),
                                   ("GET", "/api/sessions", None), ("POST", "/api/auth/logout", None)]:
            assert request(method, path, body, session, forged)[0] == 403, (method, path)
elif mode == "logout":
    session = sys.stdin.read().strip()
    assert request("POST", "/api/auth/logout", cookie=session)[0] == 204
    assert request("GET", "/api/sessions", cookie=session)[0] == 401
else:
    raise AssertionError("unknown mode")
