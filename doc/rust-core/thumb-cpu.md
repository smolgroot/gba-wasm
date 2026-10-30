# ARM7TDMI CPU

The CPU is an ARMv4T interpreter (`ThumbCpu` in `rust/src/cpu.rs`) with a high-level-emulated (HLE) BIOS. It passes the ARM and Thumb suites from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests) (see [Testing Strategy](./testing-strategy.md)); that is evidence of instruction-level correctness, not of compatibility with any particular game.

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
- `LDM`/`STM` with an empty list transfers PC and moves the base by `0x40`. A stored base is the original value only when it is the first register in the list. A loaded base overrides writeback.
- Thumb `POP {pc}` and ARM loads into PC do not change state (no ARMv5 interworking).
- `CMP`/`CMN`/`TST`/`TEQ` with `Rd = pc` and S set copy SPSR to CPSR without writing PC. In User and System mode, which have no SPSR, CPSR is left unchanged.

Encodings that are undefined on ARMv4T (the NV condition, coprocessor instructions, ARMv5 forms), `MSR` writes that change the T bit or select an invalid mode, and unpredictable PC/base combinations return `UnsupportedArmInstruction`/`UnsupportedInstruction`. The error carries the address, state, and opcode, and leaves PC at the faulting instruction. Undefined-instruction exceptions are not raised.

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
