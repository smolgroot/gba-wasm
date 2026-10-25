# GPU Pipeline

The current GPU milestone implements GBA Mode 3: a 240 x 160 bitmap with 16-bit RGB555 pixels stored in VRAM. This provides a deterministic first rendering path before adding tile, background, and object modes.

## Current representation

The `GpuCore` stores a `framebuffer` of RGBA pixel data with dimensions matching the GBA screen: 240 x 160.

## Capabilities

- clear the framebuffer
- set a single pixel
- decode RGB555 pixels from VRAM
- expose RGBA framebuffer bytes to the browser through WebAssembly

## Design intent

The GPU model intentionally stays close to the raw framebuffer instead of embedding browser logic. This makes the Rust implementation testable and reusable when the full GBA pipeline is implemented.
