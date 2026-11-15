#!/usr/bin/env python3
"""Save and apply per-device LE connection parameters on Barrot adapters."""

import argparse
import json
import os
from pathlib import Path
import re
import socket
import struct
import time

CONFIG = Path(__file__).resolve().parent / "local/connection-parameters.json"
LOAD_CONNECTION_PARAMETERS = 0x0035


def validate(device):
    if not re.fullmatch(r"(?:[0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}", device["address"]):
        raise ValueError("Invalid Bluetooth device address")
    if device["address_type"] not in ("public", "random"):
        raise ValueError("Address type must be public or random")
    minimum, maximum, latency, timeout = (
        device[name] for name in ("min_interval", "max_interval", "latency", "timeout")
    )
    if any(type(value) is not int for value in (minimum, maximum, latency, timeout)):
        raise ValueError("Connection parameters must be integers")
    if not (6 <= minimum <= maximum <= 3200 and 0 <= latency <= 499 and 10 <= timeout <= 3200):
        raise ValueError("Connection parameters exceed Bluetooth limits")
    # Interval units are 1.25 ms, timeout units are 10 ms; the timeout must
    # exceed twice the largest interval including peripheral latency.
    if timeout * 8 <= 2 * maximum * (latency + 1):
        raise ValueError("Supervision timeout is too short for this interval")
    return device


def load_config():
    if not CONFIG.exists():
        return []
    config = json.loads(CONFIG.read_text())
    if config["version"] != 1 or not isinstance(config["devices"], list):
        raise ValueError("Unsupported connection-parameter configuration")
    return [validate(device) for device in config["devices"]]


def packet(index, device):
    validate(device)
    address = bytes.fromhex(device["address"].replace(":", ""))[::-1]
    address_type = 1 if device["address_type"] == "public" else 2
    payload = struct.pack("<H6sBHHHH", 1, address, address_type,
                          device["min_interval"], device["max_interval"],
                          device["latency"], device["timeout"])
    return struct.pack("<HHH", LOAD_CONNECTION_PARAMETERS, index, len(payload)) + payload


def barrot_adapters():
    for controller in sorted(Path("/sys/class/bluetooth").glob("hci*")):
        if not re.fullmatch(r"hci[0-9]+", controller.name):
            continue
        for parent in controller.resolve().parents:
            if (parent / "idVendor").exists() and (parent / "idProduct").exists():
                if ((parent / "idVendor").read_text().strip() == "33fa" and
                        (parent / "idProduct").read_text().strip() == "0010"):
                    yield int(controller.name[3:])
                break


def apply():
    devices = load_config()
    if not devices:
        return
    if os.geteuid() != 0:
        raise ValueError("Applying link parameters requires sudo")
    adapters = list(barrot_adapters())
    if not adapters:
        raise ValueError("No Barrot 33fa:0010 adapter found")
    for index in adapters:
        with socket.socket(socket.AF_BLUETOOTH, socket.SOCK_RAW, socket.BTPROTO_HCI) as connection:
            # Linux HCI control channel (3), no particular HCI device (0xffff).
            connection.bind((0xffff, 3))
            for device in devices:
                connection.send(packet(index, device))
                deadline = time.monotonic() + 5
                while True:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise TimeoutError("No reply to Load Connection Parameters")
                    connection.settimeout(remaining)
                    reply = connection.recv(4096)
                    if len(reply) < 9:
                        continue
                    event, controller, _ = struct.unpack_from("<HHH", reply)
                    opcode, status = struct.unpack_from("<HB", reply, 6)
                    if event in (1, 2) and controller == index and opcode == LOAD_CONNECTION_PARAMETERS:
                        if status:
                            raise ValueError(f"Kernel rejected link parameters: status {status:#x}")
                        break
                print(f"hci{index}: applied saved link parameters for {device['address']}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    remember = commands.add_parser("remember", help="save the tested trainer profile locally")
    remember.add_argument("address")
    remember.add_argument("--address-type", choices=("public", "random"), required=True)
    commands.add_parser("apply", help="apply saved profiles using Linux's management interface")
    args = parser.parse_args()
    try:
        if args.action == "remember":
            device = validate(dict(address=args.address.upper(), address_type=args.address_type,
                                   min_interval=20, max_interval=80, latency=0, timeout=1350))
            devices = [entry for entry in load_config() if
                       (entry["address"].upper(), entry["address_type"]) !=
                       (device["address"], device["address_type"])]
            devices.append(device)
            CONFIG.parent.mkdir(parents=True, exist_ok=True)
            CONFIG.write_text(json.dumps({"version": 1, "devices": devices}, indent=2) + "\n")
            print(f"Saved trainer link profile to {CONFIG}")
        else:
            apply()
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"Link parameters: {error}\n")
