# Compile and flash!

In this section, you'll be able to compile your firmware and flash it to your microcontroller.

## 1. Compile the firmware

To compile the firmware is easy, just run

```shell
cargo build --release
```

If you've done all the previous steps correctly, you can find your compiled firmware at `target/<your_target>/release` folder.

If you encountered any problems when compiling the firmware, please report it [here](https://github.com/HaoboGu/rmk/issues).

## 2. Flash the firmware

The last step is to flash compiled firmware to your microcontroller. This needs a debug probe like [daplink](https://daplink.io/), [jlink](https://www.segger.com/products/debug-probes/j-link/) or stlink(stm32 only). If you've get your debug probe, connect it with your board and host, make sure you have installed [probe-rs](https://probe.rs/), then just run

```shell
cargo run --release
```

The firmware will be flashed to your microcontroller and the firmware will run automatically, yay!

For more configurations of RMK, you can checkout feature documentations on the left.