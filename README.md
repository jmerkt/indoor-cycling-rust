# Indoor Cycling

A Rust desktop application for running indoor cycling workouts and controlling a
smart trainer.

I started this project because I wanted an application that could run on my
Raspberry Pi and control my indoor trainer without depending on a commercial
training platform.

> [!WARNING]
> This is an early-stage personal project and has been coded almost entirely
> with LLMs. Do not expect high-quality, carefully hand-crafted code.

## Features

- Discover and connect compatible Bluetooth Low Energy devices.
- Assign separate devices as a power source, secondary power source, heart-rate
  monitor, cadence sensor, or smart trainer.
- Display live power, cadence, heart rate, ride time, and ride statistics.
- Control a compatible FTMS smart trainer in ERG mode.
- Dynamically generate workouts from a set of parameters:
  - duration and workout type;
  - effort and recovery intensity;
  - effort and recovery duration;
  - workout variation.
- Scale workout targets from the configured FTP.
- Pause a workout and move between workout stages.
- Save completed rides as FIT activity files.
- Persist application settings and the latest generated workout.

Device communication currently supports Bluetooth only. ANT/ANT+ is not
supported.

## Requirements

- A recent stable Rust toolchain, installed with
  [rustup](https://rustup.rs/).
- A graphical Linux environment.
- BlueZ running on the system for Bluetooth support.

On Ubuntu, Debian, and Raspberry Pi OS, install the native build and runtime
dependencies with:

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config libfontconfig1-dev libxkbcommon-x11-0 bluez
```

## Run in development

From the repository root:

```sh
cargo run -p indoor-cycling-app
```

## Build and run a release

Build the optimized binary:

```sh
cargo build --release -p indoor-cycling-app
```

Run it:

```sh
./target/release/indoor-cycling-app
```

The release binary can also be copied to another machine with the same
architecture and compatible system libraries.

## Test

```sh
cargo test --workspace
```

## Local data

On Linux, the application stores its settings in
`~/.config/indoor-cycling/settings.json` and saved rides in
`~/.local/share/indoor-cycling/rides/`. The standard `XDG_CONFIG_HOME` and
`XDG_DATA_HOME` environment variables override those base directories.

## Workspace

- `crates/core`: ride samples, statistics, and workout generation/state.
- `crates/devices`: Bluetooth device discovery, measurements, and trainer
  control.
- `crates/storage`: settings persistence and ride export.
- `crates/app`: the Slint graphical application.

## License

This project is licensed under the GNU General Public License v3.0 only.
