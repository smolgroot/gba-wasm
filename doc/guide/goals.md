# Project Goals

## Core direction

The project is designed to make a GBA emulator accessible in the browser while keeping the emulator logic robust and future-proof.

## Functional goals

- render a GBA framebuffer in the browser
- expose Rust-based state to the UI layer through a thin wasm bridge
- support responsive controls and a PWA-friendly app shell
- preserve emulator behavior through explicit memory and timing models

## Engineering goals

- keep the Rust crate independent from React or browser code
- use tiny, focused tests for subsystem correctness
- maintain a stable WebAssembly API surface for the frontend
- document architecture and architecture decisions in public docs
