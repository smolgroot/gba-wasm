# GPU Pipeline

The GPU subsystem is modeled around a framebuffer and pixel-level operations. This keeps the render path deterministic and simple while providing a stable foundation for future GBA video modes and object rendering.

## Current representation

The `GpuCore` stores a `framebuffer` of RGBA pixel data with dimensions matching the GBA screen: 240 x 160.

## Capabilities

- clear the framebuffer
- set a single pixel
- render a test pattern
- expose raw pixel bytes to the browser

## Design intent

The GPU model intentionally stays close to the raw framebuffer instead of embedding browser logic. This makes the Rust implementation testable and reusable when the full GBA pipeline is implemented.
