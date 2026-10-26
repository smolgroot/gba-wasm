# GPU Core

The GPU core selects the modeled display mode from `DISPCNT` and currently renders GBA Mode 3 pixels from VRAM into an RGBA framebuffer.

## Data structure

The `GpuCore` stores a `framebuffer` as a flat RGBA array of length `SCREEN_WIDTH * SCREEN_HEIGHT * 4`.

## Operations

- `clear()` resets the framebuffer
- `set_pixel(x, y, rgba)` writes an RGBA pixel into the framebuffer
- `render(memory)` reads `DISPCNT`; Mode 3 renders RGB555 VRAM, while unsupported modes currently produce an opaque black frame
- `render_mode3(memory)` reads 240 x 160 little-endian RGB555 pixels from VRAM

Each 5-bit color channel is expanded to 8 bits, and output pixels receive a fully opaque alpha channel.

## Browser integration

The Rust core owns VRAM and display-control state. The browser receives only the resulting framebuffer bytes for canvas rendering.

## Test coverage

Unit tests cover exact RGB555 decoding, `DISPCNT` mode selection, and the opaque-black fallback for unsupported modes.
