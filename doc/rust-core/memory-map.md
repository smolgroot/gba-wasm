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

The current implementation keeps memory regions as contiguous byte arrays in `MemoryMap`. The `read_u8` and `write_u8` methods map GBA-style addresses into storage regions. The 1 KiB modeled I/O window is visible only at `0x04000000` through `0x040003FF`; this currently includes the display-control register at `0x04000000`. Reads from other unimplemented addresses return zero and writes are ignored, a temporary approximation rather than open-bus emulation. The VRAM aperture mirrors the final 32 KiB into addresses `0x06018000` through `0x0601FFFF`.

Mode 3 uses the first 76,800 bytes of VRAM as a 240 x 160 array of little-endian RGB555 pixels.

Each cartridge ROM window (`0x08000000`, `0x0A000000`, and `0x0C000000`) maps the same loaded image at offset zero. Reads past the loaded image return `0xFF`; the image is not repeated by its own length. Images are limited to 32 MiB, the size of one ROM window. ROM writes are ignored; backup RAM and cartridge-specific wait states are not implemented.

## Validation

Rust tests cover EWRAM access, direct VRAM byte access, the mirrored VRAM aperture, ROM window mapping, and reads beyond a loaded image.
