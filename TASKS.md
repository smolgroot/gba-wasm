# Pecten Development Backlog

This is the canonical implementation backlog. Each story defines observable acceptance criteria so work can be implemented and verified in small increments. Update its status when work starts or finishes; keep architectural detail in the linked documentation rather than duplicating it here.

## Status and priority

- `DONE`: acceptance criteria are met by the current implementation and tests.
- `IN PROGRESS`: work has started; acceptance criteria are not all met yet.
- `NEXT`: recommended next work item; do not start dependent stories before this is complete.
- `BACKLOG`: planned, not yet selected.
- `P0`: blocks the first credible homebrew boot milestone.
- `P1`: needed for useful application behavior or broader compatibility.
- `P2`: later compatibility and quality work.

A passing test suite does not imply general commercial-ROM compatibility. Do not claim a game boots unless it has been tested and its behavior is documented.

## Current capabilities

- `DONE` Rust/WASM build and browser integration.
- `DONE` GBA-sized RAM/video storage, cartridge validation and windows, a generated ARM Mode 3 fixture, and a stateful browser frame loop.
- `DONE` ARMv4T ARM/Thumb interpreter passing the jsmolka/gba-tests ARM, Thumb, and memory suites, with IRQ exception delivery through a generated HLE BIOS image. This is not a real BIOS implementation.
- `DONE` Display status and VBlank/HBlank/VCount IRQs, timers, keypad interrupts, DMA (immediate/VBlank/HBlank), and VBlank-aligned frame stepping.
- `DONE` Scanline PPU for all display modes, with objects, windows, and blending.
- `DONE` SRAM, Flash, and EEPROM saves persisted in the browser; pause/resume; keyboard and touch input.
- `DONE` PSG and DirectSound audio streamed to the browser at 32,768 Hz.
- `DONE` Robustness: seeded fuzz tests show random cartridge code, random I/O writes, and corrupted save states produce errors, never core panics (which abort the WASM instance). Frontend tests run with Vitest (`pnpm test`).

Wait states, the prefetch buffer, serial/link hardware, the cartridge RTC and other GPIO devices, and several BIOS services (BIOS-02) are not implemented.

## Roadmap

Work the stories in this order. Measurement comes before timing work: wait states and the prefetch buffer change every game's behavior and cost performance, so the mgba-suite score (TEST-01), the local smoke run (COMPAT-02), and a performance baseline (PERF-01) must exist first to show whether a change helps or regresses.

| Order | Story | Priority | Why now |
| --- | --- | --- | --- |
| 1 | [CPU-04](#cpu-04-accept-every-encoding-the-arm7tdmi-executes) Accept every encoding the ARM7TDMI executes | `P1` | A rejected Thumb `BX` encoding stops a ROM in its first frames; one-line class of bug. |
| 2 | [TEST-01](#test-01-run-the-mgba-test-suite-in-cargo-test) Run the mGBA test suite in `cargo test` | `P1` | Gives a per-suite pass count to drive and verify all later accuracy work. |
| 3 | [COMPAT-02](#compat-02-local-smoke-run-over-a-rom-directory) Local smoke run over a ROM directory | `P1` | Catches crashes in real games without committing ROMs. |
| 4 | [BIOS-02](#bios-02-close-hle-bios-compatibility-gaps) Close HLE BIOS gaps | `P1` | Unsupported SWIs (notably `SoftReset`) end the game. |
| 5 | [CART-01](#cart-01-emulate-the-cartridge-real-time-clock) Cartridge real-time clock | `P1` | Pokémon Ruby/Sapphire/Emerald time events depend on it. |
| 6 | [PERF-01](#perf-01-measure-and-protect-emulation-speed) Measure and protect emulation speed | `P1` | Some ROMs run at ~1.3× real time natively; timing work will cost more. |
| 7 | [TIME-02](#time-02-memory-wait-states) Memory wait states | `P1` | Largest accuracy gap: every access costs the same today. |
| 8 | [TIME-03](#time-03-gamepak-prefetch-buffer) GamePak prefetch buffer | `P2` | Completes cartridge timing once TIME-02 lands. |
| 9 | [DMA-01](#dma-01-implement-dma-and-interrupt-delivery) DMA timing and video capture | `P2` | Builds on TIME-02 access costs. |
| 10 | [ROM-03](#rom-03-model-reads-beyond-the-rom-image) Reads beyond the ROM image | `P2` | Small; checked by test ROMs and some games. |
| 11 | [CPU-05](#cpu-05-undefined-instruction-exception) Undefined-instruction exception | `P2` | Hardware behavior instead of a fatal error. |
| 12 | [SAVE-03](#save-03-per-game-overrides) Per-game overrides | `P2` | Save type and RTC for ROMs whose strings mislead detection. |
| 13 | [COMPAT-01](#compat-01-track-a-reproducible-homebrew-compatibility-set) Homebrew compatibility set and CI | `P2` | Runs the suites from TEST-01/TEST-02 on every push. |
| 14 | [TEST-02](#test-02-add-further-redistributable-test-roms) Further test ROMs | `P2` | Widens coverage once the harness pattern exists. |
| 15 | [PPU-03](#ppu-03-close-ppu-fidelity-gaps) PPU fidelity gaps | `P2` | Affine/bitmap mosaic, object cycle limits, mid-line changes. |
| 16 | [AUDIO-02](#audio-02-close-audio-fidelity-gaps) Audio fidelity gaps | `P2` | `SOUNDBIAS` resolution, band limiting. |
| 17 | [INPUT-02](#input-02-gamepad-input) Gamepad input | `P2` | Frontend only; independent of core work. |
| 18 | [UI-02](#ui-02-rewind) Rewind | `P2` | Builds on save states. |
| 19 | [BOOT-03](#boot-03-optional-user-provided-bios) Optional user-provided BIOS | `P2` | Reference for HLE behavior; unlocks `bios.gba`. |
| 20 | [CHEAT-01](#cheat-01-cheat-codes) Cheat codes | `P2` | |
| 21 | [CART-02](#cart-02-other-gpio-devices) Other GPIO devices | `P2` | Solar, gyro, tilt, rumble: few titles. |
| 22 | [LINK-01](#link-01-serial-port-and-link-cable) Serial port and link cable | `P2` | Large; needs a transport design. |
| 23 | [DBG-01](#dbg-01-developer-views) Developer views | `P2` | Memory, tile, OAM, and I/O viewers. |

Frontend stories (INPUT-02, UI-02) can run in parallel with the core stories at any point.

## Completed stories

### BOOT-00: Execute a synthetic ARM/Thumb boot slice

**Status:** `DONE`

Acceptance criteria met:

- Initialize the HLE runner in ARM System mode at the cartridge entry address with an IWRAM stack.
- Execute a tested ARM branch and immediate `MOV`, and switch to Thumb state through `BX`.
- Preserve the narrow Thumb demo and report unsupported opcodes with their address and value.
- Verify the browser WASM call with built-in ARM and Thumb demos and cover the CPU behavior with Rust tests.

This story proves the instruction-state handoff only; it does not establish boot compatibility with arbitrary ROMs.

## P0: Bootable homebrew foundation

### ROM-01: Validate cartridge images

**Status:** `DONE`  
**Priority:** `P0`

As a user, I want the emulator to reject invalid or unsupported cartridge files before execution, so failures are understandable and do not look like emulator crashes.

Acceptance criteria:

- Accept a bounded, non-empty ROM byte buffer and reject empty, truncated-header, or over-limit input with a user-visible error.
- Parse and report the Nintendo logo/checksum/header fields needed to identify the cartridge; validation errors must identify the failing field.
- Do not silently repeat arbitrary file bytes across cartridge windows; implement documented ROM-window behavior for the supported image size and define reads beyond it.
- Include unit tests for valid header fixtures, malformed headers, and ROM-window boundary reads.
- Keep test fixtures homebrew or synthetic; do not add copyrighted commercial ROM images to the repository.

### CPU-01: Correctly model CPU execution state

**Status:** `DONE`  
**Priority:** `P0`  
**Depends on:** ROM-01

As a developer, I want ARM7TDMI register, status, and execution-mode behavior modeled explicitly, so instruction implementations share correct CPU semantics.

Acceptance criteria:

- Represent ARM and Thumb state through CPSR T-bit and use the correct aligned instruction fetch width for each state.
- Implement condition evaluation, PC operand semantics, and flag updates as shared tested behavior rather than duplicating approximations in instruction handlers.
- Add tests for all ARM condition codes, N/Z/C/V preservation and updates, PC reads, and alignment at ARM/Thumb transitions.
- Unsupported instructions return the instruction address, state, and opcode without mutating unrelated registers or memory.
- Keep instruction execution deterministic and separate from browser code.

### CPU-02: Expand ARM startup instruction coverage

**Status:** `DONE`
**Priority:** `P0`  
**Depends on:** CPU-01

As a user loading a homebrew cartridge, I want common ARM startup code to execute, so a real cartridge entry point can initialize memory and transition into game code.

Acceptance criteria:

- Implement and test ARM data-processing instructions used by the selected homebrew fixture, including immediate rotation, operand shifts, and required flag behavior.
- Implement required ARM single data transfer operations and PC-relative literal loads with correct addressing modes.
- Implement conditional execution and branch/link behavior for supported encodings.
- Use a legally distributable homebrew test ROM (or a generated fixture from source) and record its source, expected behavior, and tested instruction path.
- The ROM runs to a known checkpoint without an unsupported-opcode error; unsupported opcodes remain errors, not no-ops.

### BOOT-01: Define the BIOS and HLE boot contract

**Status:** `DONE`  
**Priority:** `P0`  
**Depends on:** CPU-01, CPU-02

As a developer, I want an explicit boot contract, so HLE startup is not mistaken for full BIOS compatibility.

Acceptance criteria:

- Document initial CPU mode, CPSR, banked/system stack state, and entry PC for each supported boot path.
- Define whether the project supports a user-provided BIOS image, HLE-only boot, or both; do not imply BIOS equivalence unless verified.
- Implement only the BIOS services required by the selected homebrew fixture and return explicit errors for unsupported services.
- Verify the fixture reaches its documented entry/checkpoint with each supported boot path.

HLE-only boot is documented in [HLE BIOS](doc/rust-core/bios.md#boot-contract). The jsmolka/gba-tests ROMs reach their idle-loop checkpoint through it.

### BUS-01: Implement hardware-visible memory and I/O behavior

**Status:** `DONE`
**Priority:** `P0`  
**Depends on:** ROM-01, CPU-01

As a homebrew program, I want CPU reads and writes to reach the correct GBA memory and hardware registers, so initialization code observes expected device state.

Acceptance criteria:

- Decode all currently claimed memory regions without using modulo arithmetic that aliases unrelated hardware addresses.
- Implement tested byte, halfword, and word accesses, including alignment and device-specific write behavior required by the CPU and boot fixture.
- Make display control and required status/timer registers observable through I/O reads and writes.
- Test region boundaries, mirrors, ROM read-only behavior, and unimplemented address behavior.
- Document any open-bus or unmapped-address approximation rather than returning plausible but undocumented values.

### PPU-01: Render a homebrew-compatible display mode

**Status:** `DONE`  
**Priority:** `P0`  
**Depends on:** BUS-01

As a homebrew program, I want display-control registers and VRAM to determine the displayed frame, so pixels are produced by emulated hardware state instead of a test pattern.

Acceptance criteria:

- Select the active display mode from emulated display control state.
- Preserve the tested Mode 3 RGB555 rendering path and add the additional bitmap/tile mode required by the chosen homebrew fixture.
- The display reads emulated VRAM, palette, and OAM as required; the browser does not synthesize graphics input.
- Compare output against a deterministic known frame or pixel assertions from the fixture.

The fixture is jsmolka/gba-tests (Mode 4). Its "All tests passed" screen is checked by pixel bounds and frame hash.

### PPU-02: Render tile modes, objects, windows, and blending

**Status:** `DONE`
**Priority:** `P1`
**Depends on:** PPU-01, TIME-01

As a user, I want games that use tiled backgrounds and sprites to display, because nearly all commercial and most homebrew software uses them.

Acceptance criteria:

- Render text and affine backgrounds, objects, windows, and color effects from emulated state.
- Render each line with the state at its HBlank so that raster effects work.
- Cover each feature with focused pixel tests, including a raster-effect test driven through `step_frame`.
- Document the features that are not modeled.

Details are in [GPU Core](doc/rust-core/gpu-core.md). Object cycle limits, affine/bitmap mosaic, and mid-line changes are not modeled.

### TIME-01: Add bounded machine-cycle progression

**Status:** `DONE`
**Priority:** `P0`  
**Depends on:** CPU-01, BUS-01, PPU-01

As a developer, I want instructions and display events to advance on a shared deterministic clock, so programs that wait for display state can progress correctly.

Acceptance criteria:

- Each implemented instruction reports or consumes documented cycle counts.
- Advance scanline, VBlank, and frame state deterministically from emulated cycles.
- Add tests for frame/scanline boundaries and display status transitions.
- Provide a bounded frame-step API so the browser can request work without an unbounded synchronous loop.

### ROM-02: Boot the first real homebrew cartridge

**Status:** `DONE`  
**Priority:** `P0`  
**Depends on:** ROM-01, CPU-02, BOOT-01, BUS-01, PPU-01, TIME-01

As a user, I want to load a known homebrew GBA ROM and see its intended screen, so the project demonstrates a real cartridge running rather than an instruction demo.

Acceptance criteria:

- Select one legally redistributable homebrew ROM with documented provenance and expected startup behavior.
- It reaches its expected screen or a documented interaction checkpoint without unsupported instructions, invalid memory accesses, or a hang.
- The run can be repeated deterministically and produces a recorded screenshot or pixel-test result.
- Browser status distinguishes loading, running, paused, and emulation errors.
- Record the exact supported fixture and known limitations; do not generalize one passing game into broad compatibility.

The `arm.gba`, `thumb.gba`, and `memory.gba` test ROMs (MIT, provenance in `rust/tests/fixtures/gba-tests/README.md`) boot deterministically to their pixel-tested result screen. The browser run-state indicator distinguishes LOADING, RUNNING, PAUSED, and ERROR.

## P1: Usable emulator shell

### UI-01: Add pause, resume, and bounded frame stepping

**Status:** `DONE`  
**Priority:** `P1`  
**Depends on:** TIME-01

Acceptance criteria:

- Pause stops CPU and PPU progression without losing emulator state.
- Resume continues from the same state.
- Browser work is scheduled in bounded frame batches and does not block input for long intervals.
- Tests cover pause/resume state and repeated frame-step requests.

Pause is a button and the `P` key. The frame loop runs the frames that are due at each animation frame (at most six), averaging 59.73 fps times the game speed (see [WASM Boundary](doc/architecture/wasm-boundary.md#frame-loop)). Rust tests show a paused-and-resumed run matches an uninterrupted one frame for frame, and that each `step_frame` ends at the next VBlank. A headless Chrome check confirmed the frame stays frozen while paused and resumes afterwards.

### INPUT-01: Map keyboard and touch controls

**Status:** `DONE`  
**Priority:** `P1`  
**Depends on:** BUS-01

Acceptance criteria:

- Map keyboard and responsive touch controls to the GBA key input register.
- Press and release transitions are represented correctly, including simultaneous buttons.
- Focus loss and pointer cancellation cannot leave buttons stuck.
- Add unit tests for input-bit mapping and browser interaction checks for touch controls.

Keyboard input (`src/keypad.ts`) was checked in headless Chrome. Vitest tests cover the KEYINPUT bit mapping, simultaneous keys, release on blur and page hide, the D-pad sectors, and the touch controls (`src/TouchControls.test.tsx`): multi-touch, sliding between buttons, the D-pad joystick, pointer cancellation, and release on blur, page hide, and unmount. The touch tests run in jsdom with a fixed stub layout, so real CSS hit areas are not covered.

### SAVE-01: Support cartridge backup storage

**Status:** `DONE`  
**Priority:** `P1`  
**Depends on:** ROM-01, BUS-01

Acceptance criteria:

- Identify supported save type from cartridge metadata or an explicit user setting.
- Implement one save type at a time with focused read/write tests.
- Persist saves locally using a versioned browser storage format and handle quota or corrupt-data errors.
- Never modify the uploaded ROM bytes.

SRAM and Flash (64/128 KiB) are detected from SDK library strings, emulated with Rust tests, and persisted per ROM hash in versioned IndexedDB records (see [WASM Boundary](doc/architecture/wasm-boundary.md)). A headless Chrome run confirmed a Flash save is stored and restored. EEPROM (512 bytes/8 KiB) was added next, with its size inferred from the first DMA request.

### SAVE-02: Save states and save file transfer

**Status:** `DONE`
**Priority:** `P1`
**Depends on:** SAVE-01

As a user, I want to save and restore my exact position at any moment and move save files between browsers or emulators.

Acceptance criteria:

- Snapshot and restore the whole machine with a versioned format that rejects states from other cartridges or format versions without changing the running game.
- Prove completeness: a restored state must replay identically to the original run.
- Loading a state never changes the game's own save file.
- Export and import raw `.sav` files with size validation and confirmation before replacing a save.
- Confirm saves visibly.

See [WASM Boundary](doc/architecture/wasm-boundary.md#save-states).

### BOOT-02: Play a boot animation before the cartridge starts

**Status:** `DONE`
**Priority:** `P1`
**Depends on:** BOOT-01, PPU-02

As a user, I want a short Pecten logo animation when a game starts, drawn by the emulated console rather than overlaid by the browser.

Acceptance criteria:

- Draw the animation by programming emulated palette, VRAM, OAM, and display registers (bitmap background, affine logo object, brightness fades); the browser does not synthesize the frames.
- After the animation, the game runs exactly as after a direct HLE boot (same frames and save state).
- A newly pressed button skips it; a setting turns it off; a save state taken during it is the game's start state.
- Do not present it as the Nintendo BIOS boot sequence.

It replaces the browser splash ([HLE BIOS](doc/rust-core/bios.md#boot-animation)). `rust/tests/boot_intro.rs` covers the drawn frames, the key skip, the save state, and equivalence with a direct boot on `arm.gba`.

## P2: Compatibility and fidelity

### CPU-03: Complete the ARM7TDMI instruction set needed by targets

**Status:** `IN PROGRESS`  
**Priority:** `P2`  
**Depends on:** CPU-01, CPU-02

Acceptance criteria:

- Add instruction families driven by failing homebrew compatibility tests, not an unverified checklist.
- Validate flags, shifts, exceptions, banked registers, and load/store behavior with instruction-level tests.
- Add an external CPU test corpus where licensing permits and record unsupported families.

All ARMv4T instruction classes are implemented, and the jsmolka/gba-tests ARM and Thumb suites pass. Undefined-instruction exceptions and coprocessor encodings are reported as errors.

### DMA-01: Implement DMA and interrupt delivery

**Status:** `IN PROGRESS`  
**Priority:** `P2`  
**Depends on:** BUS-01, TIME-01

Acceptance criteria:

- Model DMA channels, transfer widths, timing triggers, and register side effects required by selected target ROMs.
- Deliver interrupt requests through CPU exception state with tested priority and return behavior.
- Add deterministic tests for transfers, trigger conditions, and interrupt entry/return.

Immediate, VBlank, and HBlank DMA and IRQ entry/return through the BIOS dispatcher are implemented and tested. Sound FIFO DMA is implemented ([Audio](doc/rust-core/apu.md)). Remaining: video capture timing and validation against selected target ROMs. After TIME-02, charge transfers the wait states of their source and destination regions, model the start-up delay and CPU/DMA interleaving, and verify with the TEST-01 DMA suite.

### AUDIO-01: Implement GBA audio output

**Status:** `DONE`  
**Priority:** `P2`  
**Depends on:** BUS-01, TIME-01

Acceptance criteria:

- Implement sound register behavior and channel state required by target ROMs.
- Mix samples at a documented rate and stream through browser audio without blocking emulation.
- Test deterministic sample generation and audio lifecycle behavior.

All four PSG channels and both DirectSound FIFOs, including sound DMA, are implemented and mixed at 32,768 Hz ([Audio](doc/rust-core/apu.md)). Rust tests cover sample generation. `src/audio.test.ts` runs the worklet processor directly (prebuffering, fade-out on underrun and while paused, the latency bound, playback rate, ring wrap) and checks the `AudioOutput` lifecycle against a fake Web Audio API (resume only while suspended, cleanup when the worklet fails to load, teardown). The App's gesture listeners were checked by hand in headless Chrome.

### CPU-04: Accept every encoding the ARM7TDMI executes

**Status:** `NEXT`
**Priority:** `P1`
**Depends on:** CPU-03

As a user, I want ROMs that contain unusual but valid encodings to keep running instead of stopping with `UnsupportedInstruction`.

Acceptance criteria:

- Thumb high-register `BX` (`0x4700`-`0x477F`) branches whatever bit 7 (H1) and bits 0-2 hold, as the ARM7TDMI ignores them. `0x470A` at `0x080A19EA` stops Pokémon Silver Moon (a ROM hack, not vendored) at frame 13 today.
- Audit every decoder path that returns `UnsupportedInstruction`/`UnsupportedArmInstruction` for a valid encoding and record, per path, either the hardware behavior with a reference or why the error is correct (for example: genuinely undefined, coprocessor).
- Add a unit test for each relaxed encoding, using synthetic opcodes.

### TEST-01: Run the mGBA test suite in `cargo test`

**Status:** `BACKLOG`
**Priority:** `P1`
**Depends on:** ROM-02

As a developer, I want the [mgba-emu/suite](https://github.com/mgba-emu/suite) test ROM (MIT) scored automatically, so accuracy work is measured against the reference emulator's own tests rather than by playing games.

The suite has no prebuilt release, and it has no run-all mode. A menu lists its suites in this order: memory, I/O read, timing, timers, timer IRQ, shifter, carry, multiply long, BIOS math, DMA, SIO read, SIO timing, misc. edge case, video. Pressing A runs a suite. When the mGBA debug registers answer, the suite logs `BEGIN: <name>` and `END: <passed>/<total>` through them (`0x04FFF780` enable: write `0xC0DE`, read back `0x1DEA`; `0x04FFF600` 256-byte string buffer; writing `0x04FFF700` with bit 8 set emits the buffer at log level `value & 7`). It also writes text lines to SRAM.

Acceptance criteria:

- Build `suite.gba` from a pinned commit with the repository's `docker-build.sh` (devkitARM). Vendor the binary, the MIT license, and a README recording the commit and build command under `rust/tests/fixtures/mgba-suite/`, as for gba-tests.
- Implement the mGBA debug-log registers in the core as an opt-in device (off by default; enabled by a `GbaCore` setting the harness turns on). The device collects log lines for the host and does not change behavior when it is disabled; the browser build leaves it off. Unit-test the enable handshake, the string buffer, and the flush.
- Add `rust/tests/mgba_suite.rs`. For each suite it boots the ROM, navigates the menu with timed key presses through `set_pressed_keys`/`step_frame`, waits for the `END:` line within a frame budget, then parses the pass count.
- Commit the current per-suite pass counts as the baseline. The test fails when a count drops below its baseline and prints a reminder to raise the baseline when it rises, so the suite is a ratchet rather than all-or-nothing.
- Document the harness, the baseline, and the suites that cannot pass under HLE (BIOS-dependent values) in [Testing Strategy](doc/rust-core/testing-strategy.md).

### COMPAT-02: Local smoke run over a ROM directory

**Status:** `BACKLOG`
**Priority:** `P1`
**Depends on:** ROM-02

As a developer, I want one command that runs every ROM in a local directory headlessly and reports crashes, so regressions in real games are visible without committing any ROM.

Acceptance criteria:

- Add a Cargo example (for example `cargo run --release --example smoke -- <dir> [frames]`) that, for each `.gba` file, runs a fixed number of frames with a deterministic input script (periodic Start and A), and reports: the first core error with its frame, whether the frame changed, the share of non-silent audio, the detected backup type, and speed relative to real time.
- Output is a stable plain-text table suitable for diffing between commits.
- It reads ROMs only from the given path. Nothing it produces is committed except the tool itself. `rom/` stays git-ignored.
- Document it in [Testing Strategy](doc/rust-core/testing-strategy.md#commercial-roms) without naming results for commercial titles as compatibility claims.

### BIOS-02: Close HLE BIOS compatibility gaps

**Status:** `BACKLOG`
**Priority:** `P1`
**Depends on:** BOOT-01

As a user, I want games that use common BIOS services to keep running instead of stopping with `UnsupportedBiosCall`.

Acceptance criteria:

- Implement `SoftReset` (`0x00`), which many games call for their button-combination reset: it resets the stacks and the top of IWRAM and restarts at the ROM or EWRAM entry chosen by the byte at `0x03007FFA`.
- Implement `SoundBias` (`0x19`), `BitUnPack` (`0x10`), the Diff8/Diff16 unfilters (`0x16`-`0x18`), `MidiKey2Freq` (`0x1F`), and `GetBiosChecksum` (`0x0D`), each with a focused unit test.
- Make `CpuSet` and `CpuFastSet` ignore sources in the BIOS region, as the decompression services already do.
- Save the BIOS latch in save states (with a `STATE_VERSION` bump) so restored states replay identically, as SAVE-02 requires.
- Add the missing unit tests for Div, DivArm, Sqrt, and ArcTan.
- Record a decision, with a reference, for division by zero and reserved `RegisterRamReset` flags, which now stop emulation.
- Each change is driven by a named failing ROM or a documented hardware reference, not by plausible values.

### COMPAT-01: Track a reproducible homebrew compatibility set

**Status:** `BACKLOG`  
**Priority:** `P2`  
**Depends on:** ROM-02

Acceptance criteria:

- Maintain a small legally redistributable homebrew suite with source/provenance.
- Record per-ROM boot, video, input, audio, save, and known-failure status.
- Run automated instruction/frame regressions in CI where practical.
- Report compatibility per tested title and feature; do not use an unsupported universal compatibility claim.
- Add a CI workflow that runs `cargo test` (including TEST-01 and TEST-02) and `pnpm test` on every push and pull request; the deploy workflow alone does not run tests.

### CART-01: Emulate the cartridge real-time clock

**Status:** `BACKLOG`
**Priority:** `P1`
**Depends on:** BUS-01, SAVE-01

As a user playing a game with a clock chip, I want the in-game time to work, so time-based events run and the game does not report a dead battery.

Acceptance criteria:

- Model the GPIO port at `0x080000C4`-`0x080000C9` (data, direction, read-enable control) without changing ROM reads while GPIO reads are disabled.
- Model the Seiko S-3511A protocol: reset, status, date/time and time-only reads, status writes, and BCD encoding.
- Enable the RTC by detection (the `SIIRTC_V` library string) or through SAVE-03 overrides. Time comes from a host-supplied clock, so tests stay deterministic.
- Save RTC state in save states (with a `STATE_VERSION` bump) and keep the clock offset with the game's save data.
- Unit-test the serial protocol, BCD values, and the read-enable gate.

### PERF-01: Measure and protect emulation speed

**Status:** `BACKLOG`
**Priority:** `P1`
**Depends on:** COMPAT-02

As a user on a phone, I want games to run at full speed, and as a developer I want to know what each accuracy change costs.

Acceptance criteria:

- Add a benchmark that runs deterministic workloads natively (the vendored test ROMs plus a CPU-bound synthetic loop with no idle-loop skipping) and reports emulated frames per second.
- Measure the same workloads in the WASM build in a headless browser and record the ratio to native.
- Record the baseline in the docs. TIME-02 and TIME-03 must report their effect on it.
- Profile the slowest workload and record the top hot paths (instruction decode, memory dispatch, per-step event bookkeeping) as follow-up candidates. Do not optimize by changing emulated behavior.

### TIME-02: Memory wait states

**Status:** `BACKLOG`
**Priority:** `P1`
**Depends on:** TEST-01, PERF-01

As a user, I want instructions to take as long as on hardware, so games run at their real speed, including their slowdown, and timing-sensitive code behaves correctly.

Acceptance criteria:

- Charge each instruction fetch and data access the cost of its region and width: BIOS/IWRAM/I/O/OAM 1 cycle; palette and VRAM 1 cycle, with 32-bit accesses split into two halfwords; EWRAM 3 wait states; GamePak and SRAM from `WAITCNT` (WS0-WS2 first-access and sequential settings, SRAM setting).
- Distinguish non-sequential and sequential accesses as the ARM7TDMI's access sequence does, including the internal cycles already modeled.
- Writes to `WAITCNT` take effect immediately. The reset value matches the HLE boot state.
- Verify against the TEST-01 memory and timing suites. The `timing` pass count must rise and no suite may regress.
- Record the change in COMPAT-02 output and PERF-01 numbers.

### TIME-03: GamePak prefetch buffer

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** TIME-02

Acceptance criteria:

- Model the 8-halfword prefetch buffer enabled by `WAITCNT` bit 14, including how it fills during internal cycles and non-ROM accesses and how a non-sequential ROM access or branch empties it.
- Verify with the TEST-01 timing suite and document any approximation.

### ROM-03: Model reads beyond the ROM image

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** BUS-01

Acceptance criteria:

- Reads in the GamePak windows beyond the image size return the open-bus value the cartridge address bus leaves (`(address / 2) & 0xFFFF` per halfword), not `0xFF`, with a reference.
- Keep EEPROM and GPIO windows unchanged. Unit-test halfword and word reads past the end.

### CPU-05: Undefined-instruction exception

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** CPU-04

Acceptance criteria:

- Undefined ARM and Thumb encodings and coprocessor instructions enter Undefined mode through vector `0x04` with the correct banked `LR`/`SPSR`, as the hardware does.
- The HLE BIOS image handles the vector as the hardware BIOS does, documented with a reference.
- Offer an optional strict mode, kept for tests and fuzzing, that reports these as errors as today.

### SAVE-03: Per-game overrides

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** SAVE-01, CART-01

Acceptance criteria:

- Add a small table keyed by game code (header `0xAC`) that can force the save type, EEPROM size, Flash chip ID, and RTC presence. Every entry needs a reason and a source.
- Overrides take precedence over string detection. A user setting can force the save type for a ROM.
- Unit-test lookup and precedence with synthetic headers.

### TEST-02: Add further redistributable test ROMs

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** TEST-01

Acceptance criteria:

- Evaluate other test ROMs (for example the remaining jsmolka/gba-tests suites `ppu`, `save`, and `nes`, armwrestler, and hardware-test collections from other emulator projects). Vendor only those whose license permits redistribution, with provenance recorded as for gba-tests.
- Each vendored ROM gets an automated pass criterion: a result register, a debug-log line, or a frame hash of a verified-correct screen.
- Record ROMs that cannot pass under HLE (such as `bios.gba`) and which story would unblock them.

### PPU-03: Close PPU fidelity gaps

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** PPU-02, TEST-01

Acceptance criteria:

- Implement mosaic on affine and bitmap backgrounds, per-line object cycle limits with `DISPCNT` bit 5, and green swap. Each is covered by a focused pixel test.
- Decide, with a measured cost, whether to model mid-line register changes, and record the decision in [GPU Core](doc/rust-core/gpu-core.md).
- The TEST-01 video suite must not regress.

### AUDIO-02: Close audio fidelity gaps

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** AUDIO-01

Acceptance criteria:

- Model the `SOUNDBIAS` level and the amplitude resolution and sampling-rate settings.
- Band-limit PSG output so high square and noise frequencies do not alias, and document the filter.
- Keep sample generation deterministic, with tests for each change.

### INPUT-02: Gamepad input

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** INPUT-01

Acceptance criteria:

- Map the browser Gamepad API (standard mapping) to the GBA buttons, combined with keyboard and touch input.
- Disconnecting a gamepad, or losing focus, cannot leave buttons stuck.
- Vitest tests use a stubbed `navigator.getGamepads`. Allow remapping keyboard and gamepad buttons in Settings.

### UI-02: Rewind

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** SAVE-02

Acceptance criteria:

- Keep a bounded ring of periodic snapshots with a documented memory budget, and step back through them while a rewind control is held.
- Rewinding never changes the game's own save file. Audio is muted while rewinding.
- Test the ring bounds and that resuming after rewind replays identically to the original run.

### BOOT-03: Optional user-provided BIOS

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** BOOT-01

Acceptance criteria:

- Accept a user-supplied 16 KiB BIOS image, validated by size and checksum, stored locally and never bundled or uploaded.
- With it, boot from the reset vector through the real boot sequence and route `SWI` to the image. HLE remains the default.
- Use it to compare HLE services against the hardware BIOS in local runs, and run `bios.gba` when it is present.

### CHEAT-01: Cheat codes

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** BUS-01

Acceptance criteria:

- Support raw address/value patches applied each frame, then GameShark/Action Replay and CodeBreaker formats, including decryption.
- Cheats are per ROM, can be toggled, and are not part of save states unless a test shows they must be.

### CART-02: Other GPIO devices

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** CART-01

Acceptance criteria:

- Add the solar sensor, gyro, tilt sensor, and rumble on the CART-01 GPIO port, enabled through SAVE-03 overrides, each with a browser input (slider, device motion, vibration) and a unit-tested protocol.

### LINK-01: Serial port and link cable

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** TIME-02

Acceptance criteria:

- Model `SIOCNT`/`SIODATA`/`RCNT` modes (normal, multiplayer, UART, general-purpose) with transfer timing and the serial IRQ, verified by the TEST-01 SIO suites.
- With no peer connected, a transfer behaves as on hardware with nothing plugged in.
- Design the browser transport (two tabs, then network) in a separate document before implementing peers.

### DBG-01: Developer views

**Status:** `BACKLOG`
**Priority:** `P2`
**Depends on:** TEST-01

Acceptance criteria:

- Expose read-only inspection through the WASM boundary: CPU registers, memory ranges, I/O registers, tiles, palettes, OAM, and the TEST-01 debug-log lines.
- Inspection never advances or changes emulated state. Add a browser panel behind a developer setting.

## Development workflow

For each story:

1. Change its status to `IN PROGRESS` and identify the focused test that will validate it.
2. Implement the smallest coherent behavior slice and add tests alongside it.
3. Run the relevant Rust tests (`cargo test` in `rust/`) or frontend tests (`pnpm test`), then the production build when the WASM boundary changes.
4. Update the story’s acceptance criteria/status and the relevant architecture docs in the same change.
5. Mark `DONE` only when every acceptance criterion is verifiably met.
