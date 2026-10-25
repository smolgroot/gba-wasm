# ARM7TDMI Boot Slice

The current CPU milestone is a small, testable ARM/Thumb interpreter slice, not a complete ARM7TDMI implementation.

## Execution model

The cartridge runner accepts images from 0xC0 bytes through 32 MiB, validates the Nintendo logo, fixed header value, and complement checksum, and reports the title, game code, maker code, and version. It performs a minimal HLE handoff directly to `0x08000000` in ARM System mode, with the System stack initialized to `0x03007F00`. It does not execute a BIOS image or reproduce the full BIOS boot sequence. It executes a caller-specified instruction count, then renders Mode 3 from VRAM. The WebAssembly API caps a run at 1,000,000 instructions.

The separate Thumb test runner still starts directly in Thumb state. Ordinary commercial GBA ROMs are not expected to boot because the CPU, memory devices, BIOS services, interrupts, timing, and graphics hardware are incomplete.

## Implemented instructions

ARM:

- conditional `B` and `BL`
- conditional `BX`
- conditional `MSR CPSR_<fields>, Rm` with valid ARM7TDMI mode changes and banked registers; direct T-bit changes remain unsupported
- conditional immediate-offset ARM byte and word `LDR`/`STR`, including pre/post indexing, writeback, and PC-relative loads
- conditional immediate `MOV`

Thumb:

- `MOVS Rd, #imm8`
- `ADDS Rd, #imm8` and `SUBS Rd, #imm8`
- `LDR Rd, [PC, #imm]`
- immediate word `LDR` and `STR`
- conditional branches for conditions EQ through LE
- unconditional branch

Unsupported instructions return the opcode and address rather than being treated as no-ops. `MSR CPSR_<fields>, Rm` supports valid ARM7TDMI mode changes with banked register storage for FIQ, IRQ, Supervisor, Abort, and Undefined modes; User and System share a register bank. SPSR writes and CPSR writes that change the T bit remain unsupported. ARM data processing beyond immediate `MOV`, register-offset ARM transfers, Thumb byte transfers, interrupts, and cycle timing are not implemented.

The CPSR T bit selects the instruction set. ARM fetches use word-aligned addresses and Thumb fetches use halfword-aligned addresses. Architectural reads of PC use instruction address + 8 in ARM and aligned instruction address + 4 in Thumb. Unsupported-opcode errors include the execution state and leave the faulting PC and other CPU/memory state unchanged.

## Browser workflow

Use **Run ARM Boot Demo** to exercise an ARM entry branch and `MOV`, **Run Thumb Demo** to exercise the Thumb path, or load a `.gba`/`.bin` file to attempt ARM-state execution from its cartridge entry. The ARM runner rejects empty, truncated, oversized, or invalid headers and reports the parsed cartridge identity in Runtime status. Unsupported instructions are also reported there. A standard ROM may stop immediately on an unsupported startup instruction.

## Tests

Rust tests cover ARM boot state, every ARM condition against every NZCV combination, flag preservation, ARM/Thumb fetch alignment, architectural PC reads, state transitions, unsupported-opcode state preservation, Thumb literal loads, arithmetic flags, word stores, PC-relative branches, and ROM-to-VRAM-to-frame rendering.