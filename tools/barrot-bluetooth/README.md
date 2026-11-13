# Local Barrot Bluetooth workaround

This folder preserves the workaround tested with the Barrot USB adapter
`33fa:0010` on Ubuntu 26.04, kernel `7.0.0-43-generic`.

The adapter hangs on LE extended scanning (`0x2041`), resets, and loses its BlueZ
device objects. The patch adapts the proposed upstream fix by Ernesto Salvetti,
limiting the changed behavior to Barrot controllers. It disables broken extended
scanning/connection commands, avoids mixing extended advertising with legacy
scanning, and handles the controller's busy reply to an optional feature query.

The patch and helpers belong in Git. Compiled modules and downloaded kernel
sources are saved locally under `target/barrot-bluetooth/<kernel>/`, which is
already ignored by Git. They survive reboot; `cargo clean` deletes them.

## Apply after reboot

From the repository root, close the cycling app and disconnect Bluetooth devices:

```sh
sudo ./tools/barrot-bluetooth/driver.sh load
```

The script verifies the kernel version and module checksums, briefly stops
Bluetooth, loads the patched modules, and restores normal Bluetooth service
startup. Repeating `load` while the matching workaround is active does nothing.
It does not replace Ubuntu's installed modules or configure automatic loading.

Check the loaded modules with:

```sh
./tools/barrot-bluetooth/driver.sh status
```

## Rebuild after a kernel update or cargo clean

Install the headers and tools if needed:

```sh
sudo apt install build-essential python3 patch bluez usbutils kmod linux-headers-$(uname -r)
```

Build as your normal user:

```sh
python3 tools/barrot-bluetooth/build.py
```

Then run the `load` command above. The builder infers the matching Ubuntu source
tag from the headers package and downloads the Bluetooth sources from Ubuntu's
official kernel repository. It compares source headers with installed headers,
applies the patch without fuzz, and checks kernel version strings and exported
symbol ABI versions before saving modules. Building leaves the loaded drivers
unchanged. There is no download or compilation inside the administrator command.

Builds are specific to one kernel. A different kernel requires a fresh build;
the loader refuses to use an older kernel's modules. Only Ubuntu generic kernels
from the Resolute source tree are supported by this helper. If a future source
tag is unavailable or the patch no longer applies, the build stops without
replacing the previously verified modules. Do not force a mismatched module to
load. Check whether Ubuntu has incorporated the official fix instead.

An explicit source tag can be supplied for a matching installed headers package:

```sh
python3 tools/barrot-bluetooth/build.py --source-tag Ubuntu-7.0.0-43.43
```

## Restore Ubuntu's drivers

Close the cycling app and disconnect Bluetooth devices, then run:

```sh
sudo ./tools/barrot-bluetooth/driver.sh restore
```

Rebooting also restores Ubuntu's installed drivers. Once an Ubuntu kernel works
without the workaround, stop running `load`; no system installation needs to be
removed. The source folder and generated build cache can then be deleted.

## Scope and validation

These are custom kernel modules and can affect system stability. Secure Boot
was disabled on the test machine. The helpers do not disable signature checks;
with Secure Boot enabled, normal kernel module signing requirements still apply.

With the workaround loaded, Rally power/cadence and trainer power/control all
connected; power measurements arrived from both devices and the adapter reset
loop stopped. The user also confirmed connections work in the application.
Heart-rate testing was deferred. ERG/resistance commands were not part of the
diagnostic test.

The source patch is derived from Linux kernel code (GPL-2.0-only). The proposed
upstream series is credited to Ernesto Salvetti; this is a local adaptation,
not a released Ubuntu driver update.

- [Upstream adapter fix proposal](https://lore-kernel.gnuweeb.org/linux-bluetooth/20261031-barrot-le-fixes-v1-v1-3-30e2089882bf%40tambosi.tn.it/)
- [Ubuntu kernel source used for the verified build](https://git.launchpad.net/~ubuntu-kernel/ubuntu/+source/linux/+git/resolute/?h=Ubuntu-7.0.0-43.43)
