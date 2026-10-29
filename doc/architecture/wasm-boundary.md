# WASM Boundary

The WebAssembly boundary keeps the browser app and the emulator core separate.

## Exported API

- `inspect_rom(rom)` validates the cartridge header and returns its identity.
- `WasmEmulator::new(rom)` boots a cartridge through the HLE boot path ([ARM7TDMI CPU](../rust-core/thumb-cpu.md#boot-contract)).
- `step_frame()` runs until the next VBlank and returns the 240 x 160 RGBA frame. It fails with the emulation error instead of looping forever.
- `set_pressed_keys(mask)` sets the pressed buttons (bit 0 A, 1 B, 2 Select, 3 Start, 4 Right, 5 Left, 6 Up, 7 Down, 8 R, 9 L).
- `backup_type()`, `backup_data()`, `load_backup(data)`, and `take_backup_dirty()` expose cartridge save memory ([Memory Map](../rust-core/memory-map.md#cartridge-backup)).
- `take_audio_samples()` and `audio_sample_rate()` expose interleaved stereo audio at 32,768 Hz ([Audio](../rust-core/apu.md)).
- `run_rom(rom, steps)`, `run_thumb_rom(rom, steps)`, and `hello(name)` remain for the demos and smoke tests.

## Frame loop

The emulator page steps frames from `requestAnimationFrame` callbacks against a deadline. The deadline advances by one GBA frame (280,896 cycles, about 16.74 ms) divided by the game speed for each frame run, with 2 ms of tolerance for timestamp jitter. The speed is chosen in Settings: Slow ×0.5, Normal ×1, or Fast ×2. A callback runs every frame that is due, up to four, and draws only the last one, so Normal averages 59.73 fps and Fast about 120 fps on a 60 Hz display. After a stall of more than three frames, the deadline resynchronizes instead of catching up. Audio is played at the same speed as the game, so its pitch rises at Fast and drops at Slow, like a tape played at another speed.

The Sound setting is a mute toggle plus a volume slider (a `GainNode` after the worklet, ramped to avoid clicks). Speed, volume, and mute are saved with the other settings in `localStorage`. While paused (Pause button or `P`), frames are not stepped, the core state is untouched, and the deadline restarts on resume. The run-state indicator shows LOADING, RUNNING, PAUSED, or ERROR.

## Data flow

The browser loads the WASM module, sends ROM bytes, key state, and stored save data in, and receives RGBA frames and save data out. `src/wasmBridge.ts` types the generated bindings. `src/keypad.ts` maps keyboard input: arrows for the D-pad, X/Z for A/B, A/S for L/R, Enter for Start, and Shift for Select. Keys are released when the page loses focus.

`src/backupStorage.ts` stores save data in its own IndexedDB database (`gba-wasm-saves`), keyed by the SHA-256 of the ROM, as versioned records (`version`, `backupType`, `data`, `savedAt`). The emulator page persists a save once the game has stopped writing for 30 frames, when the page is hidden, and before the emulator is replaced. Records with an unknown version or a different save type are reported and not loaded. Write failures, such as storage quota, are shown in the status bar.

## Why this boundary matters

A narrow API surface reduces accidental cross-layer coupling and makes the core easier to test in Rust without browser dependencies.
