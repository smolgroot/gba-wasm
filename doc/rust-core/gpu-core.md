# GPU Core

`GpuCore` (`rust/src/gpu.rs`) is a scanline renderer. It builds each visible line from emulated I/O registers, VRAM, palette RAM, and OAM into an RGBA framebuffer (`SCREEN_WIDTH * SCREEN_HEIGHT * 4` bytes, alpha always opaque). RGB555 channels are expanded to 8 bits.

## Frame timing

`GbaCore::step_frame` renders line N once the display reaches HBlank on line N, then returns the frame when VBlank starts. Register, VRAM, or palette changes made between lines (by HBlank interrupts or HBlank DMA) therefore affect the following lines, as on hardware. `GpuCore::render` renders all 160 lines from the current state and is used by the instruction-count demo runners.

## Layers

| `DISPCNT` mode | Backgrounds |
| --- | --- |
| 0 | BG0-BG3 text |
| 1 | BG0-BG1 text, BG2 affine |
| 2 | BG2-BG3 affine |
| 3, 4, 5 | BG2 bitmap: 240 x 160 RGB555, 240 x 160 8-bit paletted (index 0 transparent), or 160 x 128 RGB555 |

- **Text backgrounds:** 256/512-pixel map sizes, 9-bit scroll offsets, 4bpp palette banks or 8bpp tiles, tile flips, and mosaic.
- **Affine backgrounds:** 128-1024-pixel maps, wraparound, and the BG2/BG3 internal reference point. That point reloads from `BGxX`/`BGxY` at line 0 and on any write to those registers, and advances by `PB`/`PD` after each line. The bitmap modes are sampled through BG2's affine transform, so the BIOS identity parameters (`PA = PD = 0x100`) give the unscaled image. Modes 4 and 5 use the frame selected by `DISPCNT` bit 4.
- **Objects:** all 12 shapes and sizes, 4bpp/8bpp, 1D/2D tile mapping, flips, affine and double-size objects, semi-transparent and OBJ-window objects, and mosaic. In bitmap modes, tiles below 512 are unavailable, as on hardware. When objects overlap, the lower OAM entry wins unless a later one has a strictly higher priority.
- **Composition:** the top two opaque layers by priority are found per pixel. Objects draw above backgrounds of the same priority, lower BG numbers win ties, and the backdrop (palette entry 0) is always last. WIN0, WIN1, the OBJ window, and the outside region select the enabled layers and effects, with WIN0 taking precedence. `BLDCNT` alpha blending, brightness increase, and brightness decrease are applied with `BLDALPHA`/`BLDY` coefficients capped at 16. Semi-transparent objects alpha-blend with a second target regardless of the blend mode.
- **Forced blank** (`DISPCNT` bit 7) displays white.

## Not modeled

- Per-line object cycle limits and `DISPCNT` bit 5 (HBlank interval free)
- Mosaic on affine and bitmap backgrounds
- Mid-line register changes
- Green swap

## Test coverage

Unit tests cover:
- the three bitmap modes, frame select, forced blank, and the backdrop fallback
- text background scrolling, wrapping, flips, and palette banks
- affine reference-point stepping and mid-frame reloads
- layer and object priority
- object size, flip, and disable flags
- window masking, darkening, and alpha and brightness arithmetic

`step_frame_renders_each_line_with_the_state_at_its_hblank` drives a repeating HBlank DMA raster effect through the frame loop. The jsmolka/gba-tests integration test checks a full Mode 4 frame by hash.
