# GBA WASM Development Backlog

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
- `DONE` GBA-sized EWRAM, IWRAM, VRAM, palette, and OAM storage with initial address decoding.
- `DONE` Mode 3 RGB555-to-RGBA framebuffer rendering.
- `DONE` A narrow ARM/Thumb instruction subset, cartridge-byte loading, HLE entry state, and browser demos.
- `DONE` Focused tests for instruction behavior, VRAM access, Mode 3 pixels, and a small ROM-to-frame path.

Current CPU support is limited to ARM `B`/`BL`, `BX`, immediate `MOV`, and the Thumb instructions documented in [the CPU reference](doc/rust-core/thumb-cpu.md). BIOS execution, most CPU instructions, hardware I/O, timing, interrupts, DMA, backup saves, and most graphics features are not implemented.

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

**Status:** `BACKLOG`  
**Priority:** `P0`  
**Depends on:** CPU-01, CPU-02

As a developer, I want an explicit boot contract, so HLE startup is not mistaken for full BIOS compatibility.

Acceptance criteria:

- Document initial CPU mode, CPSR, banked/system stack state, and entry PC for each supported boot path.
- Define whether the project supports a user-provided BIOS image, HLE-only boot, or both; do not imply BIOS equivalence unless verified.
- Implement only the BIOS services required by the selected homebrew fixture and return explicit errors for unsupported services.
- Verify the fixture reaches its documented entry/checkpoint with each supported boot path.

### BUS-01: Implement hardware-visible memory and I/O behavior

**Status:** `IN PROGRESS`
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

**Status:** `BACKLOG`  
**Priority:** `P0`  
**Depends on:** BUS-01

As a homebrew program, I want display-control registers and VRAM to determine the displayed frame, so pixels are produced by emulated hardware state instead of a test pattern.

Acceptance criteria:

- Select the active display mode from emulated display control state.
- Preserve the tested Mode 3 RGB555 rendering path and add the additional bitmap/tile mode required by the chosen homebrew fixture.
- The display reads emulated VRAM, palette, and OAM as required; the browser does not synthesize graphics input.
- Compare output against a deterministic known frame or pixel assertions from the fixture.

### TIME-01: Add bounded machine-cycle progression

**Status:** `IN PROGRESS`
**Priority:** `P0`  
**Depends on:** CPU-01, BUS-01, PPU-01

As a developer, I want instructions and display events to advance on a shared deterministic clock, so programs that wait for display state can progress correctly.

Acceptance criteria:

- Each implemented instruction reports or consumes documented cycle counts.
- Advance scanline, VBlank, and frame state deterministically from emulated cycles.
- Add tests for frame/scanline boundaries and display status transitions.
- Provide a bounded frame-step API so the browser can request work without an unbounded synchronous loop.

### ROM-02: Boot the first real homebrew cartridge

**Status:** `BACKLOG`  
**Priority:** `P0`  
**Depends on:** ROM-01, CPU-02, BOOT-01, BUS-01, PPU-01, TIME-01

As a user, I want to load a known homebrew GBA ROM and see its intended screen, so the project demonstrates a real cartridge running rather than an instruction demo.

Acceptance criteria:

- Select one legally redistributable homebrew ROM with documented provenance and expected startup behavior.
- It reaches its expected screen or a documented interaction checkpoint without unsupported instructions, invalid memory accesses, or a hang.
- The run can be repeated deterministically and produces a recorded screenshot or pixel-test result.
- Browser status distinguishes loading, running, paused, and emulation errors.
- Record the exact supported fixture and known limitations; do not generalize one passing game into broad compatibility.

## P1: Usable emulator shell

### UI-01: Add pause, resume, and bounded frame stepping

**Status:** `BACKLOG`  
**Priority:** `P1`  
**Depends on:** TIME-01

Acceptance criteria:

- Pause stops CPU and PPU progression without losing emulator state.
- Resume continues from the same state.
- Browser work is scheduled in bounded frame batches and does not block input for long intervals.
- Tests cover pause/resume state and repeated frame-step requests.

### INPUT-01: Map keyboard and touch controls

**Status:** `BACKLOG`  
**Priority:** `P1`  
**Depends on:** BUS-01

Acceptance criteria:

- Map keyboard and responsive touch controls to the GBA key input register.
- Press and release transitions are represented correctly, including simultaneous buttons.
- Focus loss and pointer cancellation cannot leave buttons stuck.
- Add unit tests for input-bit mapping and browser interaction checks for touch controls.

### SAVE-01: Support cartridge backup storage

**Status:** `BACKLOG`  
**Priority:** `P1`  
**Depends on:** ROM-01, BUS-01

Acceptance criteria:

- Identify supported save type from cartridge metadata or an explicit user setting.
- Implement one save type at a time with focused read/write tests.
- Persist saves locally using a versioned browser storage format and handle quota or corrupt-data errors.
- Never modify the uploaded ROM bytes.

## P2: Compatibility and fidelity

### CPU-03: Complete the ARM7TDMI instruction set needed by targets

**Status:** `BACKLOG`  
**Priority:** `P2`  
**Depends on:** CPU-01, CPU-02

Acceptance criteria:

- Add instruction families driven by failing homebrew compatibility tests, not an unverified checklist.
- Validate flags, shifts, exceptions, banked registers, and load/store behavior with instruction-level tests.
- Add an external CPU test corpus where licensing permits and record unsupported families.

### DMA-01: Implement DMA and interrupt delivery

**Status:** `BACKLOG`  
**Priority:** `P2`  
**Depends on:** BUS-01, TIME-01

Acceptance criteria:

- Model DMA channels, transfer widths, timing triggers, and register side effects required by selected target ROMs.
- Deliver interrupt requests through CPU exception state with tested priority and return behavior.
- Add deterministic tests for transfers, trigger conditions, and interrupt entry/return.

### AUDIO-01: Implement GBA audio output

**Status:** `BACKLOG`  
**Priority:** `P2`  
**Depends on:** BUS-01, TIME-01

Acceptance criteria:

- Implement sound register behavior and channel state required by target ROMs.
- Mix samples at a documented rate and stream through browser audio without blocking emulation.
- Test deterministic sample generation and audio lifecycle behavior.

### COMPAT-01: Track a reproducible homebrew compatibility set

**Status:** `BACKLOG`  
**Priority:** `P2`  
**Depends on:** ROM-02

Acceptance criteria:

- Maintain a small legally redistributable homebrew suite with source/provenance.
- Record per-ROM boot, video, input, audio, save, and known-failure status.
- Run automated instruction/frame regressions in CI where practical.
- Report compatibility per tested title and feature; do not use an unsupported universal compatibility claim.

## Development workflow

For each story:

1. Change its status to `IN PROGRESS` and identify the focused test that will validate it.
2. Implement the smallest coherent behavior slice and add tests alongside it.
3. Run the relevant Rust tests or app checks, then the production build when the WASM boundary changes.
4. Update the story’s acceptance criteria/status and the relevant architecture docs in the same change.
5. Mark `DONE` only when every acceptance criterion is verifiably met.
