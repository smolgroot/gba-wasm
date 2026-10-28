# GPU Core

The GPU core selects the display mode from `DISPCNT` and renders the bitmap modes (3, 4, and 5) from emulated VRAM and palette RAM into an RGBA framebuffer. It renders the whole frame at VBlank; mid-frame register changes are not visible.

## Data structure

The `GpuCore` stores a `framebuffer` as a flat RGBA array of length `SCREEN_WIDTH * SCREEN_HEIGHT * 4`.

## Operations

- `clear()` resets the framebuffer
- `set_pixel(x, y, rgba)` writes an RGBA pixel into the framebuffer
- `render(memory)` reads `DISPCNT`. Forced blank (bit 7) displays white. With BG2 enabled, Mode 3 shows 240 x 160 RGB555 pixels, Mode 4 shows 240 x 160 8-bit indices into the background palette, and Mode 5 shows a 160 x 128 RGB555 bitmap surrounded by the backdrop. Modes 4 and 5 use the frame selected by bit 4 (`0x06000000` or `0x0600A000`).
- Otherwise the frame is filled with the backdrop color (palette entry 0). Tile modes 0-2, objects, windows, mosaic, and blending are not implemented yet, so frames from games using those modes show only the backdrop.

Each 5-bit color channel is expanded to 8 bits, and output pixels receive a fully opaque alpha channel.

## Browser integration

The Rust core owns VRAM and display-control state. The browser receives only the resulting framebuffer bytes for canvas rendering.

## Test coverage

Unit tests cover RGB555 decoding, Mode 4 palette lookup and frame select, the Mode 5 bitmap area and backdrop, forced blank, and the backdrop fallback. An integration test boots the jsmolka/gba-tests ROMs through `step_frame` and checks that their Mode 4 "All tests passed" screen is rendered exactly, using pixel bounds and a frame hash.
