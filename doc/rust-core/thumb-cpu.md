# ARM7TDMI CPU

The CPU is an ARMv4T interpreter (`ThumbCpu` in `rust/src/cpu/`: `mod.rs` holds the core state and stepping, `arm.rs` and `thumb.rs` the two instruction sets, `alu.rs` the flags and shifter, and `bios_hle.rs` the SWI services) with a high-level-emulated (HLE) BIOS. It passes the ARM and Thumb suites from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests) (see [Testing Strategy](./testing-strategy.md)); that is evidence of instruction-level correctness, not of compatibility with any particular game.

## Boot

The CPU starts where the hardware BIOS hands over to the cartridge: `0x08000000` in ARM System mode, with the stacks set up by `ThumbCpu::boot_hle`. The full boot contract, the generated BIOS image, and the `SWI` services are described in [HLE BIOS](./bios.md).

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

IRQ entry branches to the BIOS vector at `0x18`, whose dispatcher calls the game's handler as on hardware ([HLE BIOS](./bios.md#generated-image)).

`GbaCore::step_frame` runs until the display enters VBlank (scanline 160), so each returned frame contains all visible lines. It fails with `InstructionBudgetExceeded` after 1,000,000 steps.

## Instruction coverage

All ARMv4T ARM and Thumb instruction classes are implemented, including multiply and multiply-long, `SWP`, `MRS`/`MSR` for CPSR and SPSR, banked registers for every mode, exception return (`MOVS pc, …` and `LDM {…, pc}^`), and user-bank `LDM`/`STM ^`. Behaviors specific to the ARM7TDMI that the test suites check are modeled:

- PC reads as instruction address + 8 (ARM) or + 4 (Thumb), and + 12 for ARM register-specified shifts and stored PC values. Only Thumb `LDR rd, [pc, #]` and `ADD rd, pc, #` word-align it.
- Misaligned `LDR` rotates the word, misaligned `LDRH` rotates the halfword by 8, and misaligned `LDRSH` loads a sign-extended byte.
- `LDM`/`STM` with an empty list transfers PC and moves the base by `0x40` ([GBATEK](https://problemkaputt.de/gbatek.htm#armopcodesmemoryblockdatatransferldmstm) ARM.11/THUMB.15). Thumb `PUSH {}`/`POP {}` follow the same rule, since they are `STMDB`/`LDMIA sp!`, as in NanoBoyAdvance's `Thumb_PushPop`. A stored base is the original value only when it is the first register in the list. A loaded base overrides writeback.
- Thumb `POP {pc}` and ARM loads into PC do not change state (no ARMv5 interworking).
- `CMP`/`CMN`/`TST`/`TEQ` with `Rd = pc` and S set copy SPSR to CPSR without writing PC. In User and System mode, which have no SPSR, CPSR is left unchanged.
- Should-be-one and should-be-zero fields are not decoded, as the ARM7TDMI selects instructions from ARM bits 27-20 and 7-4 only (the decode table mGBA and NanoBoyAdvance also use): ARM `BX` (bits 19-8), `MRS`/`MSR` (bits 19-16, 15-12, 11-0), `SWP` (bits 11-8), and `MUL`'s unused Rn (bits 15-12). Thumb `BX` ignores H1 and the Rd bits (bits 7 and 2-0); ARMv4T has no `BLX`.

### Rejected encodings

Every path that returns `UnsupportedArmInstruction`/`UnsupportedInstruction` was audited for CPU-04. The error carries the address, state, and opcode, and leaves PC at the faulting instruction. Undefined-instruction exceptions are not raised yet (CPU-05).

| Encoding | Why it is an error |
| --- | --- |
| ARM condition `NV` (`0xF`) | Reserved on ARMv3 and later; the ARMv4 manual leaves it UNPREDICTABLE. |
| ARM coprocessor space (bits 27-25 `110`, or `1110`) | The GBA has no coprocessor, so the ARM7TDMI takes the undefined-instruction trap. |
| ARM bits 27-25 `011` with bit 4 set | The architecturally undefined instruction space. |
| ARM `TST`/`TEQ`/`CMP`/`CMN` without S that are not `BX`/`MRS`/`MSR` | Undefined on ARMv4T (ARMv5 uses this space for `CLZ`, `BKPT`, and the DSP forms). |
| ARM register-shift operands with bit 7 set that are not multiply, `SWP`, or halfword transfers | The undefined part of the multiply/extra-load space. |
| ARM halfword transfer with L=0 and S=1 | `LDRD`/`STRD` exist only on ARMv5TE; undefined on ARMv4T. |
| `MUL`/`MLA`/long multiply with a PC operand, `RdHi == RdLo`; `SWP` with a PC operand | UNPREDICTABLE in the ARMv4 manual, with no documented ARM7TDMI result. |
| Load/store or `LDM`/`STM` with PC as a written-back or block base | UNPREDICTABLE. |
| `MRS`/`MSR` on SPSR in User or System mode; `MRS` into PC | UNPREDICTABLE: those modes have no SPSR. |
| `MSR` that changes the T bit or selects an invalid mode | UNPREDICTABLE on ARMv4T. |
| Thumb `0xB100`-`0xB3FF`, `0xB600`-`0xBBFF`, `0xBE00`-`0xBFFF` | Undefined on ARMv4T (ARMv5+ uses some for `BKPT` and others). |
| Thumb `0xDE00`-`0xDEFF` (`B` with condition AL) | Undefined on ARMv4T. |
| Thumb `0xE800`-`0xEFFF` | The ARMv5 `BLX` suffix, which is undefined on ARMv4T. |

`thumb_rejects_exactly_the_armv4t_undefined_encodings` steps all 65,536 Thumb opcodes and pins the rejected set to the Thumb rows above. Relaxing an UNPREDICTABLE row needs a failing ROM and a hardware reference.

## Errors

`CpuError` stops emulation instead of guessing; the browser shows its message as **Emulation stopped**.

| Error | Cause |
| --- | --- |
| `EmptyCartridge` | stepping without a loaded ROM |
| `UnsupportedArmInstruction`, `UnsupportedInstruction` | an undefined or unpredictable ARM or Thumb encoding (see above) |
| `UnsupportedBiosCall` | an `SWI` service that is not implemented ([HLE BIOS](./bios.md#not-implemented)) |
| `InstructionBudgetExceeded` | `step_frame` ran 1,000,000 steps without reaching VBlank; a safety bound so a frame can never loop forever |

## BIOS calls

`SWI` does not raise a Supervisor exception. The service runs in Rust as part of the instruction step; see [HLE BIOS](./bios.md#calling-convention) for the convention and the supported services.

## Timing

Cycle counts are per-instruction-family estimates. They do not model cartridge or device wait states or the prefetch buffer. An instruction whose condition fails takes 1 cycle. ALU operations take 1 cycle, with +1 for a register-specified shift and +2 for a PC write. Branches take 3. Single loads/stores take 3/2 (+2 for a PC load). Block transfers take the register count +2/+1. Multiplies use the ARM7TDMI early-termination `m` cycles, and `SWP` takes 4. Exception entry takes 3. BIOS call costs are listed in [HLE BIOS](./bios.md#timing).
