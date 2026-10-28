# ARM7TDMI CPU

The CPU is an ARMv4T interpreter (`ThumbCpu` in `rust/src/cpu.rs`) with a high-level-emulated (HLE) BIOS. It passes the ARM and Thumb suites from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests) (see [Testing Strategy](./testing-strategy.md)); that is evidence of instruction-level correctness, not of compatibility with any particular game.

## Boot contract

The only supported boot path is HLE. No BIOS image is loaded or executed, and the project does not claim BIOS equivalence. `GbaCore::load_rom` validates the cartridge header, clears I/O state and the video clock, and leaves the CPU where the hardware BIOS hands over to the cartridge:

| State | Value |
| --- | --- |
| PC | `0x08000000` (cartridge entry), ARM state |
| CPSR | `0x0000001F`: System mode, IRQ and FIQ enabled, flags clear |
| SP (System/User) | `0x03007F00` |
| SP (IRQ) | `0x03007FA0` |
| SP (Supervisor) | `0x03007FE0` |
| Other registers and SPSRs | `0` |

The separate `run_thumb_rom` demo starts at `0x08000000` in Thumb System mode without header validation.

## Execution model

`step_with_cycles` performs one unit of work and advances the video clock and timers by its cycle estimate. In priority order:

1. Run a pending DMA transfer (the CPU is stalled; see [Memory Map](./memory-map.md)).
2. If halted, skip directly to the next display or timer event that can raise `IF`, or wake when `IE & IF != 0`.
3. Take an IRQ exception when `IME` is set, `IE & IF != 0`, and CPSR.I is clear: enter IRQ mode with `LR_irq = next instruction + 4`, save CPSR to `SPSR_irq`, set CPSR.I, and branch to `0x18`.
4. At the head of a confirmed idle loop, skip to the next event (see below).
5. Execute one ARM or Thumb instruction.

### Idle-loop skipping

Games often wait for VBlank by polling a RAM flag that their IRQ handler sets, which costs tens of thousands of instructions per frame. A short backward branch (at most 32 bytes) marks a candidate loop. The loop is confirmed idle when all three hold:
- Every instruction since the last arrival at its head only changed registers and flags: ALU operations, loads, and branches without link. Stores, block transfers, PC writes other than branches, PSR transfers, SWIs, and calls cancel it.
- No register that changes every cycle (timer counters, `SOUNDCNT_X`) was read.
- Two consecutive arrivals see identical registers and CPSR.

Such a loop cannot exit until something outside the CPU changes, so time advances directly to the next display or timer event. One real iteration then runs before the next skip, so changes to polled registers such as `VCOUNT` are always seen. DMA transfers and IRQ entry cancel the detection. A loop exits within one iteration of the event, as it would without skipping. This made Super Mario Advance 4 about 2.2 times faster.

The generated BIOS image at `0x00000000` contains only the exception vectors and the hardware IRQ dispatcher (save `r0-r3, r12, lr`, call the handler pointer at `0x03007FFC`, restore, `SUBS pc, lr, #4`), so game IRQ handlers run as on hardware. Other vectors are idle loops.

`GbaCore::step_frame` runs until the display enters VBlank (scanline 160), so each returned frame contains all visible lines. It fails with an error after 1,000,000 steps.

## Instruction coverage

All ARMv4T ARM and Thumb instruction classes are implemented, including multiply and multiply-long, `SWP`, `MRS`/`MSR` for CPSR and SPSR, banked registers for every mode, exception return (`MOVS pc, …` and `LDM {…, pc}^`), and user-bank `LDM`/`STM ^`. Behaviors specific to the ARM7TDMI that the test suites check are modeled:

- PC reads as instruction address + 8 (ARM) or + 4 (Thumb), and + 12 for ARM register-specified shifts and stored PC values. Only Thumb `LDR rd, [pc, #]` and `ADD rd, pc, #` word-align it.
- Misaligned `LDR` rotates the word, misaligned `LDRH` rotates the halfword by 8, and misaligned `LDRSH` loads a sign-extended byte.
- `LDM`/`STM` with an empty list transfers PC and moves the base by `0x40`. A stored base is the original value only when it is the first register in the list. A loaded base overrides writeback.
- Thumb `POP {pc}` and ARM loads into PC do not change state (no ARMv5 interworking).
- `CMP`/`CMN`/`TST`/`TEQ` with `Rd = pc` and S set copy SPSR to CPSR without writing PC. In User and System mode, which have no SPSR, CPSR is left unchanged.

Encodings that are undefined on ARMv4T (the NV condition, coprocessor instructions, ARMv5 forms), `MSR` writes that change the T bit or select an invalid mode, and unpredictable PC/base combinations return `UnsupportedArmInstruction`/`UnsupportedInstruction`. The error carries the address, state, and opcode, and leaves PC at the faulting instruction. Undefined-instruction exceptions are not raised.

## HLE BIOS services

`SWI` takes its service number from the low 8 bits in Thumb and from bits 23-16 in ARM, matching the BIOS convention.

| SWI | Service | Notes |
| --- | --- | --- |
| `0x01` | RegisterRamReset | Memory regions and the serial, sound, and other register groups selected by the low byte. Reserved flags are rejected. |
| `0x02` | Halt | Waits for `IE & IF != 0`. |
| `0x04` | IntrWait | Sets `IME`, then waits until the game's IRQ handler sets a requested bit in the BIOS flags at `0x03007FF8`, and clears it. With `r0 = 1`, previously set flags are discarded first. |
| `0x05` | VBlankIntrWait | `IntrWait` with `r0 = 1, r1 = 1`. |
| `0x06`, `0x07` | Div, DivArm | `r0` quotient, `r1` remainder, `r3` absolute quotient. Division by zero, which hangs real hardware, is reported as an unsupported call. |
| `0x08` | Sqrt | Integer square root. |
| `0x09` | ArcTan | The BIOS polynomial; r1 and r3 receive its intermediate values. |
| `0x0A` | ArcTan2 | Full-turn angle (0x0000-0xFFFF) of (r0, r1), using the BIOS quadrant rules. |
| `0x0B` | CpuSet | Halfword/word copy or fixed-source fill. |
| `0x0C` | CpuFastSet | Word count rounded up to whole 8-word blocks. |
| `0x0E` | BgAffineSet | Matrix plus reference point from the texture and screen centers. |
| `0x0F` | ObjAffineSet | Matrices written with the stride in `r3`. Sine/cosine come from a 1.14 fixed-point table and may differ from the BIOS table by one unit. |
| `0x11`, `0x12` | LZ77UnCompWram/Vram | The VRAM variant writes halfwords. |
| `0x13` | HuffUnComp | 4- and 8-bit data units, written as words. |
| `0x14`, `0x15` | RLUnCompWram/Vram | The VRAM variant writes halfwords. |

Any other service, or a wait with an empty interrupt mask, returns `UnsupportedBiosCall`.

## Timing

Cycle counts are per-instruction-family estimates. They do not model cartridge or device wait states or the prefetch buffer. ALU operations take 1 cycle, with +1 for a register-specified shift and +2 for a PC write. Branches take 3. Single loads/stores take 3/2 (+2 for a PC load). Block transfers take the register count +2/+1. Multiplies use the ARM7TDMI early-termination `m` cycles. Exception entry takes 3. `CpuSet` costs 3 + 2 per unit and `CpuFastSet` 3 + 2 per word; other HLE services cost 3.
