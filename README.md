# Indoor Cycling Rust

An early Rust workspace for an indoor cycling application.

The first milestone is intentionally small: a desktop app with a simulated
trainer, a simple workout engine, live ride metrics, and a place to grow device
support later.

## Workspace

- `crates/core`: ride samples, statistics, and workout state.
- `crates/devices`: trainer abstraction plus a simulated trainer.
- `crates/storage`: local ride export helpers.
- `crates/app`: Slint GUI application.

## Run

On Ubuntu/Debian, install the native build dependencies first:

```sh
sudo apt-get install build-essential pkg-config libfontconfig1-dev libxkbcommon-x11-0
```

```sh
cargo run -p indoor-cycling-app
```

## Test

```sh
cargo test
```

## Next Steps

- Add ride saving from the GUI.
- Add a rolling power chart.
- Add real FTMS Bluetooth trainer discovery behind the `TrainerDevice` trait.
- Package the app for Linux and Windows.
