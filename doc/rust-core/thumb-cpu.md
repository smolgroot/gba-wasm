# ARM7TDMI Boot Slice

The current CPU milestone is a small, testable ARM/Thumb interpreter slice, not a complete ARM7TDMI implementation.

## Execution model

The cartridge runner maps input bytes into the ROM windows and performs a minimal HLE handoff directly to `0x08000000` in ARM System mode, with the System stack initialized to `0x03007F00`. It does not execute a BIOS image or reproduce the full BIOS boot sequence. It executes a caller-specified instruction count, then renders Mode 3 from VRAM. The WebAssembly API caps a run at 1,000,000 instructions.

The separate Thumb test runner still starts directly in Thumb state. Ordinary commercial GBA ROMs are not expected to boot because the CPU, memory devices, BIOS services, interrupts, timing, and graphics hardware are incomplete.

## Implemented instructions

ARM:

- conditional `B` and `BL`
- conditional `BX`
- conditional immediate `MOV`

Thumb:

- `MOVS Rd, #imm8`
- `ADDS Rd, #imm8` and `SUBS Rd, #imm8`
- `LDR Rd, [PC, #imm]`
- immediate word `LDR` and `STR`
- conditional branches for conditions EQ through LE
- unconditional branch

Unsupported instructions return the opcode and address rather than being treated as no-ops. ARM data processing beyond immediate `MOV`, most ARM loads/stores, Thumb byte transfers, register-offset transfers, interrupts, and cycle timing are not implemented.

## Browser workflow

Use **Run ARM Boot Demo** to exercise an ARM entry branch and `MOV`, **Run Thumb Demo** to exercise the Thumb path, or load a `.gba`/`.bin` file to attempt ARM-state execution from its cartridge entry. Unsupported instructions are reported in Runtime status. A standard ROM may stop immediately on an unsupported startup instruction.

## Tests

Rust tests cover ARM boot state, ARM branch and BX mode switching, Thumb literal loads, arithmetic flags, word stores, PC-relative branches, unsupported opcodes, and ROM-to-VRAM-to-frame rendering.