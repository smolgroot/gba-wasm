# Memory Model

The Game Boy Advance memory map is the foundation that the CPU, DMA, and devices share. `MemoryMap` (`rust/src/memory.rs`) owns every region as a byte array and routes each access by address. The address-level details are in [Memory Map](../rust-core/memory-map.md).

## Regions

- BIOS: 16 KiB generated HLE image ([HLE BIOS](../rust-core/bios.md))
- EWRAM: 256 KiB external work RAM
- IWRAM: 32 KiB internal work RAM
- I/O: 1 KiB of hardware registers
- Palette: 1 KiB color table
- VRAM: 96 KiB video RAM
- OAM: 1 KiB object attribute memory
- Cartridge ROM: up to 32 MiB, in three mirrored windows
- Cartridge backup: SRAM, Flash, or EEPROM save memory

## Read and write behavior

The memory abstraction exposes `read_u8`/`write_u8`, `read_u16`/`write_u16`, and `read_u32`/`write_u32`. Wide accesses align down to their width, and the CPU applies the ARM7TDMI rotation for misaligned loads. Regions mirror across their address ranges, video memory follows its 16-bit bus rules for byte stores, and ROM writes are ignored.

I/O registers are stored in the I/O array with their documented read and write masks, and devices react to writes: timers, DMA, the keypad, interrupts, display status, and the APU.

## Owned devices

`MemoryMap` also owns the devices that are clocked with memory: the display timing, the four timers, the four DMA channels, the APU, and the cartridge backup. CPU work advances them through `advance_video_cycles`, so a single deterministic clock drives the whole machine.

## Approximations

- Memory wait states and the prefetch buffer are not modeled; instruction timings are estimates ([Timing](../rust-core/thumb-cpu.md#timing)).
- Unmapped addresses and write-only registers read `0` instead of open-bus values.
- BIOS read protection is not modeled.
