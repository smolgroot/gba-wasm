# Testing Strategy

The Rust core follows a small, explicit testing strategy tuned for emulator work.

## Principles

- verify each subsystem with a narrow unit test
- keep tests deterministic and pure
- prefer byte-accurate checks over broad assertions
- validate rendering state before integrating with the browser

## Current coverage

Unit tests live next to the code in `rust/src`. They cover the memory map, I/O devices (display status, interrupts, timers, keypad, DMA), CPU instructions and ARM7TDMI edge cases, IRQ entry and return through the [HLE BIOS](./bios.md), BIOS services, cartridge header validation and backup memory, the renderers ([GPU Core](./gpu-core.md#test-coverage)), the APU, and save-state round trips.

Two generated fixtures in `rust/src/lib.rs` exercise the boot path. `generated_arm_mode3_startup_fixture` branches past the header, writes a red Mode 3 pixel, and enables BG2. The halt fixture checks that `SWI Halt` wakes on the exact VBlank request.

## External test ROMs

`rust/tests/gba_tests.rs` runs `arm.gba`, `thumb.gba`, and `memory.gba` from [jsmolka/gba-tests](https://github.com/jsmolka/gba-tests) (MIT; commit `a7113b6`). The binaries and license are vendored in `rust/tests/fixtures/gba-tests/`. Each ROM ends in an ARM `B .` loop with the first failed test number in `r12`, or in `r7` for `thumb.gba`. The harness requires `0`, then boots each ROM through `step_frame` and checks the rendered Mode 4 "All tests passed" screen.

`bios.gba` is not vendored. It checks values read from the real BIOS image, which an HLE BIOS cannot reproduce.

## mGBA suite

`rust/tests/mgba_suite.rs` scores [mgba-emu/suite](https://github.com/mgba-emu/suite) (MIT; commit `e694203`). The binary, license, and the exact build command are in `rust/tests/fixtures/mgba-suite/`. The project publishes no release, so the ROM is built with its devkitARM Docker image. The baselines apply to the vendored binary.

The ROM has a menu and no run-all mode. For each suite the harness boots a fresh core with the opt-in [mGBA debug-log device](./memory-map.md#mgba-debug-log) enabled, presses DOWN to reach the suite and A to run it, and steps frames until the suite logs `END: <passed>/<total>`. Every suite finishes within about 220 frames; the budget is 1,000 and the whole file takes seconds in release mode. Each suite is its own `#[test]`.

The counts are a ratchet. A suite fails when its count drops below its `baseline` in the table at the top of the file, and when the total it reports changes. When a count rises, the test prints a reminder (visible with `--nocapture`); raise the baseline in the same change that improved it. A suite that ends in a core error or never logs `END:` scores 0, and the output names the error.

| Suite | Baseline | Total | Notes |
| --- | ---: | ---: | --- |
| Memory | 1344 | 1552 | |
| I/O read | 52 | 130 | |
| Timing | 310 | 2020 | Needs wait states (TIME-02) and the prefetch buffer (TIME-03). |
| Timer count-up | 353 | 936 | |
| Timer IRQ | 0 | 90 | |
| Shifter | 140 | 140 | |
| Carry | 93 | 93 | |
| Multiply long | 52 | 72 | |
| BIOS math | 0 | 615 | Stops at `Div` by zero (`unsupported BIOS SWI 0x06`), a decision BIOS-02 records. Total counted from the source. |
| DMA | 1092 | 1244 | |
| SIO register R/W | 65 | 90 | |
| SIO timing | 0 | 4 | Serial hardware is not implemented (LINK-01). |
| Misc. edge case | 3 | 12 | |

The suite's last menu entry, Video, has no automatic run: it logs nothing and reports no count, so it is not scored. Its screens can only be judged by eye.

Reasons for individual failures have not been triaged; a suite that fails here is a lead, not a diagnosis. The suite checks some values only the real BIOS produces, so a full score is not expected under HLE.

## Robustness

The release build aborts on panic, which kills the WASM instance, so hostile input must only ever produce errors. `rust/tests/robustness.rs` drives the core with seeded pseudo-random input: random code behind a valid header, random I/O and memory writes, and truncated or corrupted save states. Any panic fails the test with its seed. For a deeper search, scale the case counts and enable overflow checks:

```sh
GBA_FUZZ_SCALE=50 RUSTFLAGS="-C overflow-checks=on" cargo test --release --test robustness
```

## Frontend tests

`pnpm test` runs Vitest in jsdom over `src/**/*.test.{ts,tsx}`: keyboard and touch input (`keypad.test.ts`, `TouchControls.test.tsx`), the audio worklet and output lifecycle (`audio.test.ts`), and the home page cover flow (`coverFlowMotion.test.ts` for the scrolling physics, `featuredGames.test.ts` for matching library ROMs to featured cartridges).

## Commercial ROMs

Commercial ROMs must never be added to the repository; `rom/` is git-ignored for local experiments. Passing tests do not imply that any commercial game boots.
