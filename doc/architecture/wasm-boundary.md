# WASM Boundary

The WebAssembly boundary keeps the browser app and the emulator core separate.

## Exported functions

The Rust crate exposes small functions such as:

- `hello(name: &str)` for smoke testing
- `make_test_frame(width, height)` for browser rendering checks
- `init()` for runtime setup

## Data flow

The browser loads the wasm module, calls exported functions, and receives raw RGBA byte arrays. The React layer then paints those bytes to a canvas without needing direct emulator internals in the UI.

## Why this boundary matters

A narrow API surface reduces accidental cross-layer coupling and makes the core easier to test in Rust without browser dependencies.
