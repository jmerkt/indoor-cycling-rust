"""Verify Linux management packet encoding and reject invalid link profiles."""
import importlib.util
from pathlib import Path
import struct
import unittest

spec = importlib.util.spec_from_file_location("parameters", Path(__file__).with_name("link-parameters.py"))
parameters = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parameters)


class LinkParametersTest(unittest.TestCase):
    def device(self):
        return dict(address="01:23:45:67:89:AB", address_type="public",
                    min_interval=20, max_interval=80, latency=0, timeout=1350)

    def test_linux_management_packet_has_one_little_endian_device_record(self):
        packet = parameters.packet(3, self.device())
        self.assertEqual(struct.unpack_from("<HHH", packet), (0x0035, 3, 17))
        self.assertEqual(packet[6:15], bytes.fromhex("0100ab896745230101"))
        self.assertEqual(struct.unpack_from("<HHHH", packet, 15), (20, 80, 0, 1350))

    def test_random_address_uses_le_random_type(self):
        device = self.device()
        device["address_type"] = "random"
        self.assertEqual(parameters.packet(0, device)[14], 2)

    def test_invalid_link_parameters_cannot_be_sent(self):
        for changes in (dict(min_interval=100), dict(max_interval=3201),
                        dict(timeout=20), dict(timeout=True), dict(latency=500),
                        dict(address="not-an-address"), dict(address_type="bredr")):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                parameters.packet(0, self.device() | changes)


if __name__ == "__main__":
    unittest.main()
