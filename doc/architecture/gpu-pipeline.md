# GPU Pipeline

Video flows in one direction: the CPU and DMA write emulated VRAM, palette RAM, OAM, and display registers; the Rust renderer turns that state into RGBA pixels; the browser copies the pixels to a canvas.

## In the core

`GbaCore::step_frame` renders each visible line once the display reaches that line's HBlank, then returns the 240 x 160 RGBA frame (`SCREEN_WIDTH * SCREEN_HEIGHT * 4` bytes) when VBlank starts. Changes made between lines, by HBlank interrupts or HBlank DMA, affect the following lines as on hardware.

The renderer supports every display mode: text and affine backgrounds (modes 0-2), the three bitmap modes (3-5), objects, windows, and color effects. The details and the features that are not modeled are in [GPU Core](../rust-core/gpu-core.md).

## In the browser

`step_frame` returns the frame as bytes. The emulator page wraps them in an `ImageData` and draws them with `putImageData` on a 240 x 160 canvas, which CSS scales to the screen. When several frames run in one display refresh (at Fast or Super speed), only the last one is drawn ([Frame loop](./wasm-boundary.md#frame-loop)).

`WasmEmulator::frame()` returns the current frame without stepping, so the page can redraw after loading a save state, even while paused. Save-state slots and the library store PNG screenshots taken from the canvas.

## Design intent

The renderer stays close to the hardware model and has no browser logic. This keeps it testable with pixel assertions in Rust and lets the browser treat video as plain bytes.
