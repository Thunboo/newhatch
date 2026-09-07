#!/usr/bin/env python3
"""Validate ingress CIDRs before writing nginx configuration. Never trust IP headers."""
import ipaddress
import os
import shutil
from pathlib import Path


def render(template, subnets, port):
    if not port.isascii() or not port.isdecimal() or not 1 <= int(port) <= 65535:
        raise ValueError("FRONTEND_PORT must be 1..65535")
    networks = [ipaddress.ip_network("127.0.0.0/8"), ipaddress.ip_network("::1/128")]
    if subnets.strip():
        for item in subnets.split(","):
            item = item.strip()
            if "/" not in item:
                raise ValueError("AUTH_ALLOWED_SUBNETS must contain comma-separated CIDRs")
            try:
                networks.append(ipaddress.ip_network(item, strict=True))
            except ValueError:
                raise ValueError("AUTH_ALLOWED_SUBNETS contains an invalid or non-canonical CIDR") from None
    rules = "\n    ".join(f"allow {network};" for network in dict.fromkeys(networks))
    return template.replace("@PORT@", str(int(port))).replace("@ALLOW_RULES@", rules + "\n    deny all;")


if __name__ == "__main__":
    try:
        config = render(Path("/etc/nginx/newhatch.conf.template").read_text(),
                        os.environ.get("AUTH_ALLOWED_SUBNETS", ""),
                        os.environ.get("FRONTEND_PORT", "8080"))
        Path("/etc/nginx/conf.d/default.conf").write_text(config)
        logo = Path("/opt/newhatch/logo.png")
        if logo.is_file():
            target = Path("/usr/share/nginx/html/logo.png")
            shutil.copyfile(logo, target)
            target.chmod(0o644)
    except ValueError as error:
        raise SystemExit(str(error)) from None
