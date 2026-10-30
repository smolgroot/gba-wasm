# WASM Boundary

The WebAssembly boundary keeps the browser app and the emulator core separate.

## Exported API

- `inspect_rom(rom)` validates the cartridge header and returns its identity.
- `WasmEmulator::new(rom)` boots a cartridge through the HLE boot path ([HLE BIOS](../rust-core/bios.md#boot-contract)).
- `step_frame()` runs until the next VBlank and returns the 240 x 160 RGBA frame. It fails with the emulation error instead of looping forever.
- `set_pressed_keys(mask)` sets the pressed buttons (bit 0 A, 1 B, 2 Select, 3 Start, 4 Right, 5 Left, 6 Up, 7 Down, 8 R, 9 L).
- `backup_type()`, `backup_data()`, `load_backup(data)`, and `take_backup_dirty()` expose cartridge save memory ([Memory Map](../rust-core/memory-map.md#cartridge-backup)).
- `save_state()`, `load_state(data)`, and `frame()` snapshot and restore the whole machine (see below) and return the current frame for redrawing after a load.
- `take_audio_samples()` and `audio_sample_rate()` expose interleaved stereo audio at 32,768 Hz ([Audio](../rust-core/apu.md)).
- `run_rom(rom, steps)`, `run_thumb_rom(rom, steps)`, and `hello(name)` remain for the demos and smoke tests.

## Frame loop

The emulator page steps frames from `requestAnimationFrame` callbacks against a deadline. The deadline advances by one GBA frame (280,896 cycles, about 16.74 ms) divided by the game speed for each frame run, with 2 ms of tolerance for timestamp jitter. The speed is chosen in Settings: Slow ×0.5, Normal ×1, Fast ×2, or Super ×4. A callback runs every frame that is due, up to six (`MAX_FRAMES_PER_CALLBACK`, leaving headroom above the four per 60 Hz refresh that Super needs), and draws only the last one, so Normal averages 59.73 fps, Fast about 120 fps, and Super about 240 fps on a 60 Hz display when the device keeps up. After a stall of more than three frames, the deadline resynchronizes instead of catching up. Audio is played at the same speed as the game, so its pitch rises at Fast and Super and drops at Slow, like a tape played at another speed.

The Sound setting is a mute toggle plus a volume slider (a `GainNode` after the worklet, ramped to avoid clicks). Speed, volume, and mute are saved with the other settings in `localStorage`. While paused (Pause button or `P`), frames are not stepped, the core state is untouched, and the deadline restarts on resume. The run-state indicator shows LOADING, RUNNING, PAUSED, or ERROR.

## Data flow

The browser loads the WASM module, sends ROM bytes, key state, and stored save data in, and receives RGBA frames and save data out. `src/wasmBridge.ts` types the generated bindings. `src/keypad.ts` maps keyboard input by physical key position (`KeyboardEvent.code`): arrows for the D-pad, X/Z for A/B, A/S for L/R, Enter for Start, and Shift for Select. Keys are released when the page loses focus. `src/TouchControls.tsx` reports the on-screen buttons as a separate mask, and the page sends the OR of both, so neither input releases the other's buttons ([Controls](../manual/controls.md)).

`src/backupStorage.ts` stores save data in its own IndexedDB database (`gba-wasm-saves`), keyed by the SHA-256 of the ROM, as versioned records (`version`, `backupType`, `data`, `savedAt`). The emulator page persists a save once the game has stopped writing for 30 frames, when the page is hidden, and before the emulator is replaced. Records with an unknown version or a different save type are reported and not loaded. Write failures, such as storage quota, are shown in the status bar.

## Save states

`GbaCore::save_state` (`rust/src/state.rs`) writes a little-endian binary snapshot: the magic `GBAWSTAT`, a format version, an FNV-1a fingerprint of the ROM, then the CPU, memory, I/O devices, APU, and GPU state (about 537 KiB). It does not include:
- the ROM and the BIOS image;
- the keys currently held, so live input wins;
- the idle-loop detector's cache;
- buffered audio;
- the contents of the cartridge save memory. Loading a state never rolls back the game's own save file. The save chip's command state is included.

`load_state` checks the magic, the version, and the ROM fingerprint, restores into a copy, and swaps it in only on success, so a rejected state leaves the running game untouched. Each struct implements `State` through `impl_state!`, which destructures the struct exhaustively: adding a field without saving or explicitly skipping it fails to compile.

A Rust test and checks on local ROMs restore a state into a fresh core and require the following frames and audio to be bit-identical to the original run. Removing even one field (`video_cycles`) made every replay diverge.

The **Saves** modal ([user guide](../manual/saves.md)) offers three state slots per ROM, each with a screenshot and timestamp; F2 quick-saves and F4 quick-loads slot 1. It also downloads the cartridge save as a raw `.sav` file. Import accepts a `.sav` file whose size matches the save memory (512 or 8,192 bytes for EEPROM). After a confirmation, it stores the file and restarts the game, because games read their save at boot. A toast confirms each save state and each time the game's own save is stored.

States live in the `states` store of the `gba-wasm-saves` IndexedDB database (version 2), keyed by `<ROM SHA-256>:<slot>`, as versioned records with the snapshot, a PNG screenshot, and `savedAt`.

Replaced emulators are freed only after the page that drives them has unmounted, so a pending animation frame or save flush can never call into freed WebAssembly memory.

## Why this boundary matters

A narrow API surface reduces accidental cross-layer coupling and makes the core easier to test in Rust without browser dependencies.
