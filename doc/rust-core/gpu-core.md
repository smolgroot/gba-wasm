# GPU Core

The GPU core is a minimal framebuffer model designed to support test-driven rendering work.

## Data structure

The `GpuCore` stores a `framebuffer` as a flat RGBA array of length `SCREEN_WIDTH * SCREEN_HEIGHT * 4`.

## Operations

- `clear()` resets the framebuffer
- `set_pixel(x, y, rgba)` writes an RGBA pixel into the framebuffer
- `write_test_pattern()` fills the screen with a generated gradient pattern

## Browser integration

The browser bridge can request the framebuffer bytes and draw them to a canvas using `ImageData`.

## Test coverage

A unit test confirms that drawing a red pixel at a specific coordinate updates the expected byte locations in the framebuffer.
