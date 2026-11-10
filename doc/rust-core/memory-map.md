# Memory Map

`MemoryMap` (`rust/src/memory/`, with DMA in `dma.rs` and timers in `timers.rs`) decodes every CPU and DMA access to the GBA's memory regions, I/O registers, and cartridge.

## Regions

| Region | Size | Purpose |
| --- | --- | --- |
| BIOS | 16 KiB | generated HLE image at `0x00000000` (vectors and IRQ dispatcher; see [HLE BIOS](./bios.md)) |
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

Each cartridge ROM window (`0x08000000`, `0x0A000000`, `0x0C000000`) maps the loaded image at offset zero, and reads past the image return `0xFF`. Images are limited to 32 MiB. ROM writes are ignored, and the ROM bytes are never modified. Cartridge wait states, BIOS read protection, and open-bus values are not modeled. Unmapped addresses read `0`, which is an approximation.

## Cartridge backup

Backup memory is mapped at `0x0E000000`-`0x0FFFFFFF` with a 64 KiB window (`rust/src/backup.rs`). The type comes from the library ID string the Nintendo SDK embeds in the ROM:

| String | Type | Size |
| --- | --- | --- |
| `SRAM_V`, `SRAM_F_V` | SRAM | 32 KiB |
| `FLASH_V`, `FLASH512_V` | Flash (reports Panasonic ID `0x1B32`) | 64 KiB |
| `FLASH1M_V` | Flash (reports Sanyo ID `0x1362`), two 64 KiB banks | 128 KiB |
| `EEPROM_V` | EEPROM, 512 bytes or 8 KiB | see below |

Flash implements the `0xAA`/`0x55` unlock sequence and these commands: ID mode, chip erase, 4 KiB sector erase, byte program (which can only clear bits), and bank select. Commands complete instantly. The backup bus is 8 bits wide: halfword and word reads repeat the addressed byte, and wide writes store only the addressed byte lane. Absent backup reads `0xFF`. `Backup::take_dirty` reports changes so the browser can persist them (see [WASM Boundary](../architecture/wasm-boundary.md)).

EEPROM is accessed bit-serially through DMA3: one bit per halfword in `0x0D000000`-`0x0DFFFFFF`, or only the top 256 bytes of that range for ROMs larger than 16 MiB. A read request is `11` + address + stop bit, followed by a 68-unit read: 4 ignored bits and the 64-bit block, most significant bit first. A write is `10` + address + 64 data bits + stop bit. The address width cannot be read from the cartridge, so it comes from the length of the first DMA request: 9 or 73 units means 6 bits (512 bytes), and 17 or 81 units means 14 bits (8 KiB). Saved data of either size also sets it. Writes complete instantly, and an idle EEPROM reads `1` (ready).

## I/O registers

The I/O window is `0x04000000`-`0x040003FF`.

- **Display:** `DISPCNT` masks reserved bit 3. `DISPSTAT` exposes VBlank (lines 160-226), HBlank (from cycle 1,006 of each 1,232-cycle line), and VCount-match status, and requests enabled VBlank/HBlank/VCount interrupts. `VCOUNT` is read-only. A frame is 228 lines.
- **Interrupts:** `IE`, `IF` (write-one-to-clear), `IME`, and `WAITCNT` have their writable masks.
- **Keypad:** `KEYINPUT` exposes ten active-low buttons driven by `set_pressed_keys`. `KEYCNT` implements OR/AND interrupt conditions.
- **Timers 0-3:** reload/counter, 1/64/256/1024 prescalers, cascade, and overflow interrupts.
- **DMA 0-3:** see below.
- **Reset groups:** `reset_io_groups` clears the `RegisterRamReset` groups independently: serial (`0x120`-`0x15F`), sound (`0x060`-`0x0AF`), and all other registers. Resetting the last group restores the BG2/BG3 identity affine parameters left by the BIOS.
- **Other registers:** all other writes are stored for the devices that consume them. `BGxCNT`, `WININ`/`WINOUT`, `BLDCNT`, `BLDALPHA`, the DMA control halfwords, and `POSTFLG` read back through their documented masks. Write-only and unimplemented registers read `0`, which is an approximation rather than open-bus emulation.

## mGBA debug log

The registers at `0x04FFF600`-`0x04FFF7FF` (message buffer, flags, enable) exist only in mGBA, and test ROMs use them to report results. `MemoryMap::set_debug_log_enabled` turns the device on for a test harness; it is off by default, the browser build never enables it, and while it is off those addresses are unmapped open bus like any other. When it is on, a ROM still has to write `0xC0DE` to `0x04FFF780` and read back `0x1DEA` before the buffer and flags respond. Writing `0x04FFF700` with bit 8 set emits the NUL-terminated buffer as a line at level `value & 7`, which `take_debug_log` hands to the host. The buffer is left unchanged by a flush. At most 4,096 undrained lines are kept (the oldest are dropped). Load a new ROM and the handshake and lines reset; the device state is not part of a save state. See [Testing Strategy](./testing-strategy.md#mgba-suite).

## DMA

Enabling a channel latches its source, destination, and count (`0` means `0x4000` units, or `0x10000` for DMA3). Immediate transfers start at once. VBlank transfers start when the display enters VBlank, and HBlank transfers start on HBlanks of visible lines only. A transfer runs to completion before the CPU's next step, and the lowest channel has priority. Destination and source address control (increment, decrement, fixed, increment/reload), 16/32-bit units, repeat, end-of-transfer interrupts, and clearing the enable bit when a non-repeating transfer finishes are modeled. GamePak sources always increment. Each transfer costs 2 cycles plus 2 per unit. DMA1 and DMA2 with special timing (start timing 3) serve the sound FIFOs ([Audio](./apu.md)). Video capture (DMA3 special timing) and Game Pak DRQ are not modeled, and those transfers never start.

## Clock

CPU work advances display state, timers, and DMA triggers through `MemoryMap::advance_video_cycles`. `vblanks_started` counts VBlank entries and defines frame boundaries. `cycles_until_next_event` returns the distance to the next HBlank, line start, or timer overflow, which lets a halted CPU or an idle loop skip time without moving an interrupt. `take_volatile_read` reports reads of registers that change between those events (timer counters and `SOUNDCNT_X`), so polling loops on them are never skipped.

## Validation

Rust unit tests cover region mirrors and boundaries, aligned and unaligned accesses, video byte-store rules, stored I/O masks, display status and interrupts, timers, keypad conditions, DMA (immediate, fixed/decrementing addresses, repeating HBlank with destination reload, VBlank lines excluded), frame counting, and next-event calculation. The `memory.gba` suite from jsmolka/gba-tests checks mirrors and video byte stores.
