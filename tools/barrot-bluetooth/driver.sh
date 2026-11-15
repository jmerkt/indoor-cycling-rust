#!/usr/bin/env bash
# Load a local, kernel-matched workaround without replacing Ubuntu's files.
set -euo pipefail
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repository_dir=$(cd -- "$script_dir/../.." && pwd)
kernel=$(uname -r)
fix_dir="$repository_dir/target/barrot-bluetooth/$kernel"
action=${1:-status}
case "$action" in
    status|load|restore) ;;
    *) echo "Usage: $0 [status|load|restore]" >&2; exit 1 ;;
esac

active() {
    local name
    for name in bluetooth btusb; do
        [[ -f "$fix_dir/modules/$name/$name.ko" && -f "/sys/module/$name/srcversion" ]] || return 1
        [[ $(cat "/sys/module/$name/srcversion") == $(modinfo -F srcversion "$fix_dir/modules/$name/$name.ko") ]] || return 1
    done
}

if [[ "$action" == status ]]; then
    if active; then
        echo "Barrot workaround active for $kernel."
    else
        echo "Saved Barrot workaround is not active for $kernel."
        if [[ ! -f "$fix_dir/kernel-release" ]]; then
            echo "No saved build. Run: python3 tools/barrot-bluetooth/build.py"
        fi
    fi
    exit 0
fi
if (( EUID != 0 )); then
    echo "Run load/restore with sudo." >&2
    exit 1
fi
if [[ "$action" == load ]]; then
    if [[ ! -f "$fix_dir/kernel-release" || $(cat "$fix_dir/kernel-release") != "$kernel" ]]; then
        echo "No build for $kernel. Run python3 tools/barrot-bluetooth/build.py first." >&2
        exit 1
    fi
    cd -- "$fix_dir"
    sha256sum --check modules.sha256
    expected_magic=$(modinfo -k "$kernel" -F vermagic bluetooth)
    for name in bluetooth btusb; do
        if [[ $(modinfo -F vermagic "$fix_dir/modules/$name/$name.ko") != "$expected_magic" ]]; then
            echo "Kernel compatibility check failed for $name." >&2
            exit 1
        fi
    done
    if active; then
        python3 "$script_dir/link-parameters.py" apply
        echo "Barrot workaround is already active for $kernel."
        exit 0
    fi
    if ! lsusb -d 33fa:0010 | grep -q '33fa:0010'; then
        echo "The Barrot 33fa:0010 adapter is not present." >&2
        exit 1
    fi
fi
if bluetoothctl devices Connected | grep -q '^Device '; then
    echo "Disconnect Bluetooth devices and close the cycling app first." >&2
    exit 1
fi
case $(systemctl is-enabled bluetooth || true) in
    masked|masked-runtime)
        echo "Bluetooth is already masked; resolve that service state before loading." >&2
        exit 1 ;;
esac
rollback() {
    trap - ERR
    echo "Attempting to restore Ubuntu's Bluetooth modules." >&2
    systemctl mask --runtime bluetooth || true
    systemctl stop bluetooth || true
    modprobe -r btusb bnep rfcomm 2>/dev/null || true
    modprobe -r bluetooth 2>/dev/null || true
    modprobe btusb || true
    systemctl unmask --runtime bluetooth || true
    systemctl start bluetooth || true
    echo "If Bluetooth is unavailable, reboot to restore the stock drivers." >&2
}
trap rollback ERR
# Prevent D-Bus activation from restarting bluetoothd during the module swap.
systemctl mask --runtime bluetooth
systemctl stop bluetooth
modprobe -r btusb bnep rfcomm
modprobe -r bluetooth
if [[ "$action" == load ]]; then
    insmod "$fix_dir/modules/bluetooth/bluetooth.ko"
    modprobe -a btintel btrtl btbcm btmtk
    insmod "$fix_dir/modules/btusb/btusb.ko"
else
    modprobe btusb
fi
systemctl unmask --runtime bluetooth
systemctl start bluetooth
if [[ "$action" == load ]]; then
    python3 "$script_dir/link-parameters.py" apply
fi
trap - ERR
if [[ "$action" == load ]]; then
    echo "Barrot workaround active for $kernel until reboot or restore."
else
    echo "Ubuntu's installed Bluetooth drivers restored."
fi
