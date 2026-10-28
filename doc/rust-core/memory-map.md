# Memory Map

The first milestone defines the addressable memory layout used by the emulator core.

## Regions

| Region | Size | Purpose |
| --- | --- | --- |
| BIOS | 16 KiB | generated HLE image at `0x00000000` (vectors and IRQ dispatcher; see [ARM7TDMI CPU](./thumb-cpu.md)) |
| EWRAM | 256 KiB | external work RAM |
| IWRAM | 32 KiB | internal work RAM |
| VRAM | 96 KiB | video memory |
| OAM | 1 KiB | object attribute memory |
| Palette | 1 KiB | palette memory |
| I/O | 1 KiB | mapped hardware registers |
| Cartridge | loaded ROM | read-only data mapped at `0x08000000` through `0x0DFFFFFF` |

## Bus behavior

`MemoryMap` keeps each region as a contiguous byte array. EWRAM, IWRAM, palette, VRAM, and OAM mirror across their 16 MiB areas, and the VRAM aperture mirrors the final 32 KiB into `0x06018000`-`0x0601FFFF`. Halfword and word accesses align down to their width; the CPU applies the ARM7TDMI rotation for misaligned loads.

Video memory has a 16-bit bus. A byte store to palette RAM, or to background VRAM (`0x06000000`-`0x0600FFFF` in tile modes, up to `0x06013FFF` in bitmap modes 3-5), writes the byte to both halves of the halfword. Byte stores to OBJ VRAM and OAM are ignored.

Each cartridge ROM window (`0x08000000`, `0x0A000000`, `0x0C000000`) maps the loaded image at offset zero, and reads past the image return `0xFF`. Images are limited to 32 MiB. ROM writes are ignored. Backup RAM, cartridge wait states, BIOS read protection, and open-bus values are not modeled. Unmapped addresses read `0`, which is an approximation.

## I/O registers

The I/O window is `0x04000000`-`0x040003FF`.

- **Display:** `DISPCNT` masks reserved bit 3. `DISPSTAT` exposes VBlank (lines 160-226), HBlank (from cycle 1,006 of each 1,232-cycle line), and VCount-match status, and requests enabled VBlank/HBlank/VCount interrupts. `VCOUNT` is read-only. A frame is 228 lines.
- **Interrupts:** `IE`, `IF` (write-one-to-clear), `IME`, and `WAITCNT` have their writable masks.
- **Keypad:** `KEYINPUT` exposes ten active-low buttons driven by `set_pressed_keys`. `KEYCNT` implements OR/AND interrupt conditions.
- **Timers 0-3:** reload/counter, 1/64/256/1024 prescalers, cascade, and overflow interrupts.
- **DMA 0-3:** see below.
- **Other registers:** all other writes are stored for the devices that consume them. `BGxCNT`, `WININ`/`WINOUT`, `BLDCNT`, `BLDALPHA`, the DMA control halfwords, and `POSTFLG` read back through their documented masks. Write-only and unimplemented registers read `0`, which is an approximation rather than open-bus emulation.

## DMA

Enabling a channel latches its source, destination, and count (`0` means `0x4000` units, or `0x10000` for DMA3). Immediate transfers start at once. VBlank transfers start when the display enters VBlank, and HBlank transfers start on HBlanks of visible lines only. A transfer runs to completion before the CPU's next step, and the lowest channel has priority. Destination and source address control (increment, decrement, fixed, increment/reload), 16/32-bit units, repeat, end-of-transfer interrupts, and clearing the enable bit when a non-repeating transfer finishes are modeled. GamePak sources always increment. Each transfer costs 2 cycles plus 2 per unit. Sound FIFO and video capture timing (start timing 3) and Game Pak DRQ are not modeled, and those transfers never start.

## Clock

CPU work advances display state, timers, and DMA triggers through `MemoryMap::advance_video_cycles`. `vblanks_started` counts VBlank entries and defines frame boundaries. `cycles_until_next_event` returns the distance to the next HBlank, line start, or timer overflow, which lets a halted CPU skip idle time without moving an interrupt.

## Validation

Rust unit tests cover region mirrors and boundaries, aligned and unaligned accesses, video byte-store rules, stored I/O masks, display status and interrupts, timers, keypad conditions, DMA (immediate, fixed/decrementing addresses, repeating HBlank with destination reload, VBlank lines excluded), frame counting, and next-event calculation. The `memory.gba` suite from jsmolka/gba-tests checks mirrors and video byte stores.
