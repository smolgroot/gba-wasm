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

## Implementation notes

The current implementation keeps these regions as contiguous byte arrays in the `MemoryMap` structure. The `read_u8` and `write_u8` methods map GBA-style addresses into the correct storage regions.

## Validation

The memory map is covered by Rust unit tests that exercise read and write operations for external work RAM.
