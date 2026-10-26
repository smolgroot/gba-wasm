# Testing Strategy

The Rust core follows a small, explicit testing strategy tuned for emulator work.

## Principles

- verify each subsystem with a narrow unit test
- keep tests deterministic and pure
- prefer byte-accurate checks over broad assertions
- validate rendering state before integrating with the browser

## Current coverage

The milestone includes tests for:

- memory map read/write semantics
- GPU pixel drawing within a framebuffer
- Thumb instruction execution and unsupported-opcode reporting
- ARM HLE entry state, branch, and ARM-to-Thumb exchange
- ROM-to-VRAM-to-frame integration
- a generated, non-copyrighted ARM Mode 3 startup fixture that branches past the GBA header, writes a red VRAM pixel, enables `DISPCNT` Mode 3 and BG2, and reaches a deterministic frame after eight instructions

The fixture source is `generated_arm_mode3_startup_fixture` in `rust/src/lib.rs`. Its tested path is ARM `B`, `MOV`, word `STR`, and halfword `STRH`; the assertion checks the Mode 3 register, final PC, and first framebuffer pixel.

## Recommended next additions

- CPU instruction decode tests
- memory timing tests
- sprite and background object behavior
- framebuffer comparison snapshots for known patterns
