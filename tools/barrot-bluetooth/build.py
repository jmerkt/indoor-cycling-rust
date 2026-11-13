#!/usr/bin/env python3
"""Build the Barrot workaround from the matching Ubuntu kernel sources."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import urllib.parse
import urllib.request

TOOLS = Path(__file__).resolve().parent
REPOSITORY = TOOLS.parent.parent
SOURCE = "https://git.launchpad.net/~ubuntu-kernel/ubuntu/+source/linux/+git/resolute/"


def output(*command):
    return subprocess.check_output(command, text=True).strip()


def symbols(path):
    return {line.split()[1]: line.split()[0] for line in path.read_text().splitlines()}


def build(kernel, source_tag):
    if not re.fullmatch(r"[0-9][0-9A-Za-z.+-]*-generic", kernel):
        raise RuntimeError("This helper supports Ubuntu generic kernels only.")
    common = kernel.removesuffix("-generic")
    headers = Path("/usr/src") / ("linux-headers-" + kernel)
    if not (headers / "Module.symvers").is_file():
        raise RuntimeError(f"Install linux-headers-{kernel} first.")
    if source_tag is None:
        version = output("dpkg-query", "-W", "-f=${source:Version}", headers.name)
        source_tag = "Ubuntu-" + version
    query = "?" + urllib.parse.urlencode({"h": source_tag})
    root = REPOSITORY / "target" / "barrot-bluetooth" / kernel
    # Build separately: failed downloads, patches, or builds leave the last
    # verified modules available for reloading after reboot.
    work = root / "work"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    for suffix in ("", "-generic"):
        name = "linux-headers-" + common + suffix
        shutil.copytree(Path("/usr/src") / name, work / name, symlinks=True)

    source = work / "source"
    bluetooth = source / "net/bluetooth"
    btusb = source / "drivers/bluetooth"
    bluetooth.mkdir(parents=True)
    btusb.mkdir(parents=True)
    with urllib.request.urlopen(SOURCE + "tree/net/bluetooth" + query, timeout=45) as response:
        listing = response.read().decode()
    names = sorted(set(re.findall(r"/tree/net/bluetooth/([^?'\"/<>]+)", listing)))
    names = [name for name in names if name.endswith((".c", ".h")) or name == "Makefile"]
    if len(names) < 30:
        raise RuntimeError(f"Cannot find the expected Bluetooth sources at {source_tag}.")
    tasks = [("net/bluetooth/" + name, bluetooth / name) for name in names]
    tasks += [("drivers/bluetooth/" + name, btusb / name) for name in
              ("btusb.c", "btintel.h", "btbcm.h", "btrtl.h", "btmtk.h")]
    tasks += [("include/net/bluetooth/" + name, source / "include/net/bluetooth" / name)
              for name in ("hci.h", "hci_core.h")]

    def fetch(task):
        remote, destination = task
        destination.parent.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(SOURCE + "plain/" + remote + query, timeout=45) as response:
            destination.write_bytes(response.read())

    print(f"Downloading matching Ubuntu sources: {source_tag}", flush=True)
    with ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(fetch, tasks))
    local_headers = work / ("linux-headers-" + common) / "include/net/bluetooth"
    for name in ("hci.h", "hci_core.h"):
        if (source / "include/net/bluetooth" / name).read_bytes() != (local_headers / name).read_bytes():
            raise RuntimeError(f"Source tag and installed headers disagree: {name}")
    subprocess.run(["patch", "--batch", "--fuzz=0", "-p1", "-i",
                    str(TOOLS / "barrot-workaround.patch")], cwd=source, check=True)
    for name in ("hci.h", "hci_core.h"):
        shutil.copy2(source / "include/net/bluetooth" / name, local_headers / name)

    makefile = bluetooth / "Makefile"
    makefile.write_text("\n".join(line for line in makefile.read_text().splitlines()
                                 if not line.startswith("obj-") or
                                 line.startswith("obj-$(CONFIG_BT)")) + "\n")
    (btusb / "Makefile").write_text("obj-m := btusb.o\n")
    for directory in (bluetooth, btusb):
        subprocess.run(["make", "-C", str(work / headers.name), "M=" + str(directory),
                        "-j4", "modules"], check=True)

    expected_magic = output("modinfo", "-k", kernel, "-F", "vermagic", "bluetooth")
    installed = symbols(headers / "Module.symvers")
    for name, directory in (("bluetooth", bluetooth), ("btusb", btusb)):
        if output("modinfo", "-F", "vermagic", str(directory / (name + ".ko"))) != expected_magic:
            raise RuntimeError(f"Kernel version mismatch: {name}")
        mismatches = [name for name, crc in symbols(directory / "Module.symvers").items()
                      if installed.get(name) != crc]
        if mismatches:
            raise RuntimeError(f"Exported symbol ABI mismatch: {mismatches}")

    checksums = []
    for name, directory in (("bluetooth", bluetooth), ("btusb", btusb)):
        destination = root / "modules" / name / (name + ".ko")
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(directory / destination.name, destination)
        checksums.append(hashlib.sha256(destination.read_bytes()).hexdigest() +
                         "  " + str(destination.relative_to(root)))
    (root / "modules.sha256").write_text("\n".join(checksums) + "\n")
    (root / "kernel-release").write_text(kernel + "\n")
    (root / "source-tag").write_text(source_tag + "\n")
    print(f"Verified modules saved to {root}")
    print("No installed drivers were changed. See tools/barrot-bluetooth/README.md to load.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", default=output("uname", "-r"))
    parser.add_argument("--source-tag", help="override the Ubuntu tag inferred from the headers package")
    args = parser.parse_args()
    if os.geteuid() == 0:
        parser.exit(1, "Build as your normal user; only driver loading needs sudo.\n")
    try:
        build(args.kernel, args.source_tag)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Build stopped: {error}\n")
