# Memory Map

The first milestone defines the addressable memory layout used by the emulator core.

## Regions

| Region | Size | Purpose |
| --- | --- | --- |
| EWRAM | 256 KiB | external work RAM |
| IWRAM | 32 KiB | internal work RAM |
| VRAM | 96 KiB | video memory |
| OAM | 1 KiB | object attribute memory |
| Palette | 1 KiB | palette memory |
| I/O | 1 KiB | mapped hardware registers |
| Cartridge | loaded ROM | read-only data mapped at `0x08000000` through `0x0DFFFFFF` |

## Implementation notes

The current implementation keeps memory regions as contiguous byte arrays in `MemoryMap`. Halfword and word bus accesses align down to their access width; ARM unaligned word loads apply the architecture's rotate behavior in the CPU. The 1 KiB modeled I/O window is visible only at `0x04000000` through `0x040003FF`. `DISPCNT`, `DISPSTAT`, and read-only `VCOUNT` cover the display registers used by the bitmap path. `KEYINPUT` exposes ten active-low buttons (no input reads as `0x03FF`) and can be driven through `MemoryMap::set_pressed_keys`; `KEYCNT` configuration is stored, but keypad IRQ generation is not implemented. `IE`, `IF`, `WAITCNT`, and `IME` have their writable masks and `IF` is write-one-to-clear. Timers 0–3 implement reload/counter behavior, the 1/64/256/1024 prescalers, cascade mode, and timer overflow requests in `IF`; the CPU does not yet enter IRQ exceptions. CPU instructions advance video and timers using family-level cycle estimates, without cartridge/device wait states. Scanlines are 1,232 cycles, frames are 228 scanlines, and modeled HBlank status starts at cycle 1,006. `MemoryMap::advance_video_cycles` and `GbaCore::advance_video_cycles` also allow explicit clock advancement. Other unimplemented I/O reads return zero and writes are ignored, a temporary approximation rather than open-bus emulation. The VRAM aperture mirrors the final 32 KiB into addresses `0x06018000` through `0x0601FFFF`.

Mode 3 uses the first 76,800 bytes of VRAM as a 240 x 160 array of little-endian RGB555 pixels.

Each cartridge ROM window (`0x08000000`, `0x0A000000`, and `0x0C000000`) maps the same loaded image at offset zero. Reads past the loaded image return `0xFF`; the image is not repeated by its own length. Images are limited to 32 MiB, the size of one ROM window. ROM writes are ignored; backup RAM and cartridge-specific wait states are not implemented.

## Validation

Rust tests cover EWRAM access, direct VRAM byte access, the mirrored VRAM aperture, ROM window mapping, and reads beyond a loaded image.
