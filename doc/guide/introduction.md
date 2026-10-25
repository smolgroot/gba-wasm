# Introduction

GBA WASM is a browser-first Game Boy Advance emulation project built with Rust and WebAssembly. The project is organized around a clean boundary: the emulator logic lives in Rust, while the app shell is delivered through a modern web stack.

## Goals

The project aims to:

- run a GBA emulator in a browser using WebAssembly
- keep the emulator core deterministic and testable
- separate browser glue from the emulator logic
- support responsive UI and installable PWA behavior
- document the architecture in a static docs site hosted through GitHub Pages

## Current status

The codebase is in its first milestone: a Rust memory map and GPU framebuffer core. This foundation supports future CPU, cartridge, and timing work without forcing browser-specific concerns into the emulator core.

## Recommended workflow

1. Implement one emulator subsystem at a time in Rust
2. Add failing tests before broad feature logic
3. Expose only the minimal wasm boundary needed by the app
4. validate the browser shell separately from emulator correctness
