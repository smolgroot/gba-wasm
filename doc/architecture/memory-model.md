# Memory Model

The Game Boy Advance memory map is a foundational subsystem. The current implementation models a representative memory layout that supports the emulator core and future CPU access.

## Regions in scope

- EWRAM: 256 KiB external work RAM
- IWRAM: 32 KiB internal work RAM
- VRAM: 96 KiB video RAM
- OAM: 1 KiB object attribute memory
- palette memory: 1 KiB color table
- I/O region: mapped control registers and system state

## Read and write behavior

The core memory abstraction exposes `read_u8`, `write_u8`, `read_u16`, and `write_u16` methods. This allows the emulator to use native byte and word access patterns while the browser and Rust core remain decoupled.

## Future work

The memory model will expand to cover:

- cartridge ROM and flash mapping
- register-backed I/O regions
- memory mirroring and address aliasing
- alignment and timing-sensitive accesses
