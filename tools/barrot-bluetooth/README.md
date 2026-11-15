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

## Trainer connections dropping after about 45 seconds

A longer test found that the VANRYSEL trainer requested a 25–100 ms connection
interval with a 13.5-second supervision timeout. Linux's cached trainer profile
instead used a 30–50 ms range and a 420 ms timeout, rejected the update, and the
link repeatedly timed out after about 45 seconds. Changing adapter defaults alone
did not update this per-device cache.

Save the tested profile for your trainer, using its address from `bluetoothctl devices`:

```sh
python3 tools/barrot-bluetooth/link-parameters.py remember AA:BB:CC:DD:EE:FF --address-type public
```

Use `--address-type random` only if `bluetoothctl info <address>` identifies a
random address. This profile is intended for a trainer with the same requested
parameters; it is not a general setting for every sensor.

The address and settings are saved in `tools/barrot-bluetooth/local/`, ignored by
Git and preserved across reboots and `cargo clean`. The `driver.sh load` command
now applies saved profiles using Linux's management interface, including when
the workaround is already loaded. To apply just the profile without reloading:

```sh
sudo python3 tools/barrot-bluetooth/link-parameters.py apply
```

The setting is applied only to Barrot `33fa:0010` adapters. No BlueZ system
configuration is edited. Removing the local profile prevents future application;
restoring Ubuntu's drivers or rebooting clears the runtime override. Once an
official fix is available, test the stock driver before applying saved profiles.

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
The follow-up sustained test reproduced the trainer timeout and verified continuing
measurements after loading its requested per-device parameters. Heart-rate testing
was not part of that follow-up. ERG/resistance commands were not part of the
diagnostic test.

The source patch is derived from Linux kernel code (GPL-2.0-only). The proposed
upstream series is credited to Ernesto Salvetti; this is a local adaptation,
not a released Ubuntu driver update.

- [Upstream adapter fix proposal](https://lore-kernel.gnuweeb.org/linux-bluetooth/20261031-barrot-le-fixes-v1-v1-3-30e2089882bf%40tambosi.tn.it/)
- [Ubuntu kernel source used for the verified build](https://git.launchpad.net/~ubuntu-kernel/ubuntu/+source/linux/+git/resolute/?h=Ubuntu-7.0.0-43.43)
