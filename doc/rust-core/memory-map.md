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

The current implementation keeps memory regions as contiguous byte arrays in `MemoryMap`. Halfword and word bus accesses align down to their access width; ARM unaligned word loads apply the architecture's rotate behavior in the CPU. The 1 KiB modeled I/O window is visible only at `0x04000000` through `0x040003FF`. `DISPCNT` masks reserved bit 3, while `DISPSTAT` and read-only `VCOUNT` cover the display registers used by the bitmap path. Enabled VBlank, HBlank, and VCount events set their pending bits in `IF`. `KEYINPUT` exposes ten active-low buttons (no input reads as `0x03FF`) and can be driven through `MemoryMap::set_pressed_keys`; `KEYCNT` implements OR/AND matching and raises the keypad request when its condition becomes active. `IE`, `IF`, `WAITCNT`, and `IME` have their writable masks and `IF` is write-one-to-clear. Timers 0–3 implement reload/counter behavior, the 1/64/256/1024 prescalers, cascade mode, and timer overflow requests in `IF`; the CPU does not yet enter IRQ exceptions. CPU instructions advance video and timers using family-level cycle estimates, without cartridge/device wait states. Scanlines are 1,232 cycles, frames are 228 scanlines, and modeled HBlank status starts at cycle 1,006. `MemoryMap::advance_video_cycles` and `GbaCore::advance_video_cycles` also allow explicit clock advancement. Unimplemented I/O reads return zero and writes are ignored, a temporary approximation rather than open-bus emulation. The VRAM aperture mirrors the final 32 KiB into addresses `0x06018000` through `0x0601FFFF`.

Mode 3 uses the first 76,800 bytes of VRAM as a 240 x 160 array of little-endian RGB555 pixels.

Each cartridge ROM window (`0x08000000`, `0x0A000000`, and `0x0C000000`) maps the same loaded image at offset zero. Reads past the loaded image return `0xFF`; the image is not repeated by its own length. Images are limited to 32 MiB, the size of one ROM window. ROM writes are ignored; backup RAM and cartridge-specific wait states are not implemented.

## Validation

Rust tests cover aligned and unaligned halfword/word access, EWRAM/IWRAM/palette/OAM mirrors, I/O window boundaries and inert unimplemented registers, `DISPCNT` reserved-bit behavior, timer overflow IRQ requests, keypad OR/AND conditions, direct VRAM access and its mirrored aperture, ROM read-only mapping across windows, and reads beyond a loaded image.
