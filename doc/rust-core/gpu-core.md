# GPU Core

The GPU core renders GBA Mode 3 pixels from VRAM into an RGBA framebuffer.

## Data structure

The `GpuCore` stores a `framebuffer` as a flat RGBA array of length `SCREEN_WIDTH * SCREEN_HEIGHT * 4`.

## Operations

- `clear()` resets the framebuffer
- `set_pixel(x, y, rgba)` writes an RGBA pixel into the framebuffer
- `render_mode3(memory)` reads 240 x 160 little-endian RGB555 pixels from VRAM

Each 5-bit color channel is expanded to 8 bits, and output pixels receive a fully opaque alpha channel.

## Browser integration

The browser passes VRAM bytes to the Rust `render_mode3` WebAssembly export, then draws the returned framebuffer bytes to a canvas using `ImageData`.

## Test coverage

A unit test checks exact red, green, and blue RGB555 decoding and RGBA output. The browser currently sends a four-color VRAM fixture through this same renderer.
