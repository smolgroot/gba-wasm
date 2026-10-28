# Testing Strategy

The Rust core follows a small, explicit testing strategy tuned for emulator work.

## Principles

- verify each subsystem with a narrow unit test
- keep tests deterministic and pure
- prefer byte-accurate checks over broad assertions
- validate rendering state before integrating with the browser

## Current coverage

Unit tests live next to the code in `rust/src`. They cover the memory map, I/O devices (display status, interrupts, timers, keypad, DMA), CPU instructions and ARM7TDMI edge cases, IRQ entry and return through the HLE BIOS, BIOS services, cartridge header validation, and the bitmap renderers.

Two generated fixtures in `rust/src/lib.rs` exercise the boot path. `generated_arm_mode3_startup_fixture` branches past the header, writes a red Mode 3 pixel, and enables BG2. The halt fixture checks that `SWI Halt` wakes on the exact VBlank request.

## External test ROMs

`rust/tests/gba_tests.rs` runs `arm.gba`, `thumb.gba`, and `memory.gba` from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests) (MIT; commit `a7113b6`). The binaries and license are vendored in `rust/tests/fixtures/gba-tests/`. Each ROM ends in an ARM `B .` loop with the first failed test number in `r12`, or in `r7` for `thumb.gba`. The harness requires `0`, then boots each ROM through `step_frame` and checks the rendered Mode 4 "All tests passed" screen.

`bios.gba` is not vendored. It checks values read from the real BIOS image, which an HLE BIOS cannot reproduce.

## Commercial ROMs

Commercial ROMs must never be added to the repository; `rom/` is git-ignored for local experiments. Passing tests do not imply that any commercial game boots.
