# HLE BIOS

Pecten never loads or runs a real BIOS image. The BIOS is high-level emulated (HLE): a small generated image provides the exception vectors and the IRQ dispatcher, and BIOS calls (`SWI`) are implemented in Rust. The project does not claim BIOS equivalence, and games that depend on BIOS-specific values or timing can behave differently.

## Boot contract

The only supported boot path is HLE. An optional [boot animation](#boot-animation) can play first and then restores exactly this state. `GbaCore::load_rom` validates the cartridge header, clears I/O state to its post-BIOS values (all zero except the BG2/BG3 identity affine parameters), resets the video clock, and leaves the CPU where the hardware BIOS hands over to the cartridge (`ThumbCpu::boot_hle`):

| State | Value |
| --- | --- |
| PC | `0x08000000` (cartridge entry), ARM state |
| CPSR | `0x0000001F`: System mode, IRQ and FIQ enabled, flags clear |
| SP (System/User) | `0x03007F00` |
| SP (IRQ) | `0x03007FA0` |
| SP (Supervisor) | `0x03007FE0` |
| Other registers and SPSRs | `0` |

RAM is not cleared and holds zeros, because the core is freshly allocated for each ROM. The separate `run_thumb_rom` demo starts at `0x08000000` in Thumb System mode without header validation.

## Generated image

`bios::hle_image` (`rust/src/bios.rs`) builds the 16 KiB image mapped at `0x00000000`-`0x00003FFF`:

| Address | Content |
| --- | --- |
| `0x00`-`0x1C` | Exception vectors. All except IRQ are `B .` idle loops (reset, undefined, SWI, aborts, FIQ). |
| `0x18` | IRQ vector: branch to the dispatcher |
| `0x128` | IRQ dispatcher |
| elsewhere | zero |

The dispatcher uses the same instruction sequence as the hardware BIOS, so game IRQ handlers run as on hardware:

```txt
STMFD sp!, {r0-r3, r12, lr}
MOV   r0, #0x04000000
ADD   lr, pc, #0
LDR   pc, [r0, #-4]        ; handler pointer, mirrored at 0x03007FFC
LDMFD sp!, {r0-r3, r12, lr}
SUBS  pc, lr, #4
```

The image is not part of save states. Only code running inside the BIOS reads the image. Other reads return the BIOS latch, the last opcode the BIOS prefetched: `0xE129F000` after boot, `0xE3A02004` after any `SWI`, and the dispatcher's own opcodes after an IRQ. The latch is not saved in save states, so a restored state reads the boot value until the next BIOS call or IRQ.

## Boot animation

With the animation enabled (`GbaCore::start_boot_intro`, or the browser's **Skip boot animation** setting turned off), `step_frame` plays 116 frames (about 1.9 s) before the cartridge's first instruction. `rust/src/boot_intro.rs` draws them by programming emulated hardware, not by writing pixels:

| Element | Hardware |
| --- | --- |
| Radial gradient and wordmark | Mode 4 bitmap on BG2, ordered-dithered across 8 palette levels |
| Logo | Affine double-size 64x64 4bpp object (tile 512); `ObjAffineSet`'s matrix spins and zooms it in |
| Fades | Brightness decrease (`BLDCNT`/`BLDY`) on BG2, objects, and the backdrop |

The logo is generated from `public/logo.png` by `pnpm run bootlogo:generate` into `rust/src/boot_logo.rs`. The CPU does not run, while the video clock, timers, and APU (silent) advance a frame at a time. A newly pressed key ends the animation; keys already held when it starts do not.

At the end, `GbaCore` clears palette, VRAM, and OAM and repeats the `load_rom` I/O, clock, and GPU reset, so the game runs exactly as after a direct boot. A state saved during the animation is the game's start state, and loading any state cancels the animation. `rust/tests/boot_intro.rs` checks the drawn frames, the key skip, the save state, and that the following game frames and state match a direct boot.

This is Pecten's own animation. It does not reproduce the Nintendo BIOS boot sequence, its timing, or its sound. The cartridge header (including its logo field) is validated by `load_rom` whether or not the animation plays.

## Calling convention

`SWI` takes its service number from the low 8 bits in Thumb and from bits 23-16 in ARM, matching the BIOS convention. The CPU executes the service in Rust as a single instruction step. It does not enter Supervisor mode, so `LR_svc`, `SPSR_svc`, and the Supervisor stack are untouched and the IRQ-disable bit is not set during the call. Registers not listed as outputs below are preserved.

Unsupported services return `CpuError::UnsupportedBiosCall` with the address, state, and service number, which stops emulation with a visible error rather than continuing with wrong results.

## Services

| SWI | Service | Notes |
| --- | --- | --- |
| `0x00` | SoftReset | Reads the entry flag at `0x03007FFA`, clears `0x03007E00`-`0x03007FFF` (stacks, IRQ handler pointer, BIOS flags), zeroes r0-r12 and `LR`/`SPSR` of IRQ and Supervisor mode, resets the three boot stacks, and restarts in System mode and ARM state at EWRAM (`0x02000000`) when the flag is nonzero, otherwise at the ROM (`0x08000000`), with `LR` set to that entry ([GBATEK SWI 00h](https://problemkaputt.de/gbatek.htm#biosresetfunctions)). The flags are left clear. |
| `0x01` | RegisterRamReset | Clears the regions selected by `r0`: EWRAM, IWRAM except its top `0x200` bytes (the stacks and BIOS variables), palette, VRAM, OAM, and the serial, sound, and other register groups. Reserved flags (bits 8 and up) are rejected. |
| `0x02` | Halt | Waits for `IE & IF != 0`. |
| `0x04` | IntrWait | Sets `IME`, then waits until the game's IRQ handler sets a requested bit (`r1`) in the BIOS flags at `0x03007FF8`, and clears it. With `r0 != 0`, previously set flags are discarded first. |
| `0x05` | VBlankIntrWait | `IntrWait` with `r0 = 1, r1 = 1`. |
| `0x06`, `0x07` | Div, DivArm | `r0` quotient, `r1` remainder, `r3` absolute quotient. DivArm swaps the operands. Division by zero, which hangs real hardware, is reported as an unsupported call. |
| `0x08` | Sqrt | Integer square root. |
| `0x09` | ArcTan | The BIOS polynomial; `r1` and `r3` receive its intermediate values. |
| `0x0A` | ArcTan2 | Full-turn angle (`0x0000`-`0xFFFF`) of (`r0`, `r1`), using the BIOS quadrant rules. |
| `0x0B` | CpuSet | Halfword/word copy or fixed-source fill. |
| `0x0C` | CpuFastSet | Word count rounded up to whole 8-word blocks. |
| `0x0E` | BgAffineSet | Matrix plus reference point from the texture and screen centers. |
| `0x0F` | ObjAffineSet | Matrices written with the stride in `r3`. Sine/cosine come from a 1.14 fixed-point table and may differ from the BIOS table by one unit. |
| `0x11`, `0x12` | LZ77UnCompWram/Vram | The VRAM variant writes halfwords. |
| `0x13` | HuffUnComp | 4- and 8-bit data units, written as words. |
| `0x14`, `0x15` | RLUnCompWram/Vram | The VRAM variant writes halfwords. |
| `0x1F` | MidiKey2Freq | `r0` = `WaveData.freq / 2^((180 - r1 - r2/256) / 12)`, with the frequency read from `r0 + 4`. Computed in single-precision floating point, so the low bits may differ from the BIOS tables. |

The decompression services ignore the header's type field, as the BIOS does, and do nothing when the source lies in the BIOS region (`0x00000000`-`0x00003FFF`), as the BIOS protection does.

A wait with an empty interrupt mask, which never returns on hardware, and malformed compressed data are also reported as unsupported calls.

### Waiting

Halt and IntrWait put the CPU into a halted state that skips directly to the next display or timer event that can raise `IF` ([Execution model](./thumb-cpu.md#execution-model)). An IRQ taken during IntrWait returns to the `SWI` instruction itself, so the wait re-checks the BIOS flags after the game's handler has run, as the hardware loop does.

### Not implemented

Stop (`0x03`), GetBiosChecksum (`0x0D`), BitUnPack (`0x10`), the Diff8/Diff16 unfilters (`0x16`-`0x18`), SoundBias and the sound driver calls (`0x19`-`0x1E`, `0x20`-`0x24`, `0x28`-`0x2A`), MultiBoot (`0x25`), HardReset (`0x26`), and CustomHalt (`0x27`). Games that call them stop with an `UnsupportedBiosCall` error naming the service.

## Timing

`CpuSet` costs 3 + 2 cycles per unit and `CpuFastSet` 3 + 2 per word; other services cost 3 cycles. Real BIOS calls take far longer, so code that times BIOS routines runs faster than on hardware.

## Tests

Unit tests in `rust/src/cpu/bios_hle.rs`, `rust/src/cpu/mod.rs`, `rust/src/bios.rs`, `rust/src/memory.rs`, and `rust/src/lib.rs` cover the BIOS latch, the vector layout, IRQ entry and return through the dispatcher, Halt waking on the exact VBlank request, VBlankIntrWait running the game's IRQ handler, IntrWait with flags already set, RegisterRamReset regions and register groups (including rejected flags), CpuSet and CpuFastSet, ArcTan2 quadrants, Bg/ObjAffineSet, MidiKey2Freq, and the LZ77, Huffman, and run-length decoders. Div, Sqrt, and ArcTan have no dedicated unit tests. The jsmolka/gba-tests `bios.gba` suite is not used: it checks values read from the real BIOS image ([Testing Strategy](./testing-strategy.md)).
