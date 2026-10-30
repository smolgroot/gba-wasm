# Introduction

GBA WASM is a browser-first Game Boy Advance emulator built with Rust and WebAssembly. The project is organized around a clean boundary: the emulator logic lives in Rust, while the app shell is delivered through a modern web stack.

This section is for contributors. To play games, see the [User Manual](../manual/index.md).

## Goals

The project aims to:

- run a GBA emulator in a browser using WebAssembly
- keep the emulator core deterministic and testable
- separate browser glue from the emulator logic
- support responsive UI and installable PWA behavior
- document the architecture in a static docs site hosted through GitHub Pages

## Current status

The core runs the jsmolka/gba-tests ARM, Thumb, and memory suites and boots cartridges through a high-level-emulated BIOS ([HLE BIOS](../rust-core/bios.md)). It renders all display modes, plays PSG and DirectSound audio, and emulates SRAM, Flash, and EEPROM saves. The browser shell adds a ROM library, save states, keyboard and touch input, and PWA installation.

Not yet implemented: memory wait states and the prefetch buffer, serial and link hardware, and several BIOS services. Accuracy varies by game.

## Recommended workflow

1. Implement one emulator subsystem at a time in Rust
2. Add failing tests before broad feature logic
3. Expose only the minimal wasm boundary needed by the app
4. Validate the browser shell separately from emulator correctness

See [Development Workflow](./workflow.md) for the commands.
