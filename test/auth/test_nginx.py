import importlib.util
import ipaddress
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("configure_nginx", ROOT / "docker/configure-nginx.py")
config = importlib.util.module_from_spec(spec)
spec.loader.exec_module(config)
TEMPLATE = (ROOT / "docker/nginx.conf").read_text()


class IngressConfigTests(unittest.TestCase):
    def rules(self, subnets):
        return config.render(TEMPLATE, subnets, "8080")

    def test_empty_is_loopback_only(self):
        for subnets in ["", "  "]:
            rendered = self.rules(subnets)
            self.assertIn("allow 127.0.0.0/8;", rendered)
            self.assertIn("allow ::1/128;", rendered)
            self.assertIn("deny all;", rendered)

    def test_multiple_ipv4_ipv6_and_boundaries(self):
        rendered = self.rules("192.168.1.0/24, fd42:1234::/64,192.168.1.0/24")
        self.assertEqual(rendered.count("allow 192.168.1.0/24;"), 1)
        networks = [ipaddress.ip_network(line.strip()[6:-1]) for line in rendered.splitlines()
                    if line.strip().startswith("allow ") and "all" != line.strip()[6:-1]]
        for address, allowed in [("192.168.1.0", True), ("192.168.1.255", True), ("192.168.0.255", False),
                                 ("192.168.2.0", False), ("fd42:1234::", True),
                                 ("fd42:1234::ffff:ffff:ffff:ffff", True), ("fd42:1234:0:1::", False),
                                 ("127.42.0.1", True), ("::1", True), ("172.18.0.5", False)]:
            self.assertEqual(any(ipaddress.ip_address(address) in network for network in networks), allowed, address)

    def test_invalid_config_is_rejected(self):
        for value in ["192.168.1.100", "192.168.1.100/24", "192.168.1.0/33", "::/129", "all", "10.0.0.0/8,", "10.0.0.0/8; allow all", "not-a-cidr"]:
            with self.assertRaises(ValueError, msg=value):
                self.rules(value)
        for port in ["0", "65536", "8080;", "", "-1", " 8080"]:
            with self.assertRaises(ValueError):
                config.render(TEMPLATE, "", port)

    def test_proxy_is_loopback_without_real_ip_rewriting(self):
        rendered = self.rules("")
        self.assertIn("proxy_pass http://127.0.0.1:3000;", rendered)
        self.assertNotIn("set_real_ip_from", rendered)
        self.assertNotIn("real_ip_header", rendered)
        self.assertNotIn("@", rendered)


if __name__ == "__main__":
    unittest.main()
