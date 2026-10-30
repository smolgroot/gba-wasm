# GBA WASM

A Game Boy Advance emulator that runs in the browser. The emulator core is written in Rust and compiled to WebAssembly; the app shell is React and installs as a PWA for offline, fullscreen play on phones and computers.

## Playing games

Start with the [User Manual](./manual/index.md):

- [Getting Started](./manual/index.md): loading ROMs, the library, installing the app
- [Saved Games](./manual/saves.md): automatic saves, save states, importing and exporting `.sav` files
- [Settings](./manual/settings.md): game speed, sound, touch controls, haptics, themes
- [Desktop and Mobile Play](./manual/play-modes.md): keyboard play, portrait and landscape touch layouts
- [Controls and Shortcuts](./manual/controls.md): key bindings and shortcuts

## Current status

- ARMv4T ARM/Thumb CPU passing the jsmolka/gba-tests ARM, Thumb, and memory suites
- High-level-emulated BIOS with the common `SWI` services; no BIOS image is needed
- Scanline renderer for all display modes, with objects, windows, and blending
- PSG and DirectSound audio at 32,768 Hz
- SRAM, Flash, and EEPROM saves; save states; `.sav` import and export
- Keyboard and multi-touch input, with haptic feedback on phones

Wait states, the prefetch buffer, serial/link hardware, and several BIOS services are not implemented. Passing the test suites does not mean every game boots. The backlog is tracked in `TASKS.md` at the repository root.

## Documentation layout

1. **User Manual:** how to play, for everyone
2. **Development:** project goals and the build and test workflow
3. **Architecture:** how the emulator core, browser shell, and WebAssembly boundary fit together
4. **Rust Core:** each emulator subsystem in detail: [memory map](./rust-core/memory-map.md), [CPU](./rust-core/thumb-cpu.md), [BIOS](./rust-core/bios.md), [GPU](./rust-core/gpu-core.md), [audio](./rust-core/apu.md), and [testing](./rust-core/testing-strategy.md)

## Quick links

- [Development Workflow](./guide/workflow.md): build and test commands
- [Architecture Overview](./architecture/overview.md)
- [WASM Boundary](./architecture/wasm-boundary.md): the exported API, frame loop, and save storage
