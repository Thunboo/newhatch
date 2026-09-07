"""Build an isolated stack, exercise real TCP peers, and always clean it up."""
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
COMPOSE = ["docker", "compose", "-f", str(ROOT / "test/auth/compose.yaml")]


def client(service, host, mode, cookie=""):
    result = subprocess.run(COMPOSE + ["exec", "-T", service, "python", "/tests/client.py", host, mode],
                            input=cookie, text=True, capture_output=True)
    if result.returncode:
        # Do not print the cookie passed between clients.
        raise RuntimeError(f"{service}/{mode} failed: {result.stderr}")
    return result.stdout.strip()


try:
    subprocess.run(COMPOSE + ["up", "-d", "--build"], check=True)
    gateway_image = subprocess.check_output(COMPOSE + ["images", "-q", "gateway"], text=True).strip()
    invalid = subprocess.run(["docker", "run", "--rm", "--network", "none", "-e", "AUTH_ALLOWED_SUBNETS=invalid",
                              gateway_image, "nginx", "-t"], capture_output=True, text=True)
    assert invalid.returncode != 0 and "AUTH_ALLOWED_SUBNETS" in invalid.stderr + invalid.stdout
    for attempt in range(30):
        try:
            session = client("allowed", "192.168.1.10", "allowed")
            break
        except RuntimeError:
            if attempt == 29:
                raise
            time.sleep(1)
    client("denied", "192.168.2.10", "denied", session)
    client("bridge-client", "172.18.0.10", "denied", session)
    client("allowed", "192.168.1.10", "logout", session)
    ipv6_session = client("allowed", "fd42:1234::10", "allowed")
    client("allowed", "fd42:1234::10", "logout", ipv6_session)
    print("PASS: team IPv4/IPv6 login, me, sessions, logout; remote denial with valid cookies and forged headers; public health; loopback-only backend")
finally:
    subprocess.run(COMPOSE + ["down", "--volumes", "--remove-orphans"], check=True)
