# Architecture Overview

The project is split into a few clear concerns:

- `rust/`: emulator core logic, memory model, GPU, APU, saves, and test suite
- `src/`: browser application code and wasm bridge
- `public/`: generated wasm assets (`public/wasm/`), featured cover art, and app icons
- `doc/`: public project documentation built with VitePress
- `scripts/`: maintenance scripts, such as fetching cover art

## Separation of responsibilities

### Emulator core

The Rust crate owns simulation correctness: reading and writing memory, processing rendering state, and modeling game-system behavior. It has no dependency on the browser.

| Module | Responsibility | Docs |
| --- | --- | --- |
| `lib.rs` | `GbaCore` (frame stepping, save states) and the `wasm_bindgen` exports | [WASM Boundary](./wasm-boundary.md) |
| `cpu.rs` | ARM7TDMI interpreter and HLE BIOS services | [ARM7TDMI CPU](../rust-core/thumb-cpu.md), [HLE BIOS](../rust-core/bios.md) |
| `bios.rs` | Generated BIOS image: vectors and IRQ dispatcher | [HLE BIOS](../rust-core/bios.md) |
| `memory.rs` | Bus, I/O registers, timers, DMA, and the video clock | [Memory Map](../rust-core/memory-map.md) |
| `cartridge.rs` | Cartridge header validation | [Memory Map](../rust-core/memory-map.md) |
| `backup.rs` | SRAM, Flash, and EEPROM save memory | [Memory Map](../rust-core/memory-map.md#cartridge-backup) |
| `gpu.rs` | Scanline renderer | [GPU Core](../rust-core/gpu-core.md) |
| `apu.rs` | Sound channels and mixing | [Audio](../rust-core/apu.md) |
| `state.rs` | Save-state serialization | [WASM Boundary](./wasm-boundary.md#save-states) |

### Browser layer

The app shell owns user experience: controls, canvas presentation, input handling, and the browser-specific environment.

| Module | Responsibility |
| --- | --- |
| `App.tsx` | Routes (home, play, library), the frame loop, settings, and the Saves and Settings dialogs |
| `wasmBridge.ts` | Loads and types the generated bindings |
| `keypad.ts`, `TouchControls.tsx`, `haptics.ts` | Keyboard and touch input, haptic ticks |
| `audio.ts` | AudioWorklet output at the game speed |
| `backupStorage.ts`, `savedGames.ts` | IndexedDB storage for saves and states, and for the ROM library |
| `CoverFlow.tsx`, `coverFlowMotion.ts`, `featuredGames.ts` | Home page cover flow |
| `touchThemes.ts` | Touch-control colour themes (palettes in `App.css`) |
| `InstallPrompt.tsx` | PWA install prompt |

### WASM boundary

The boundary between the browser and the Rust core is intentionally narrow. The app loads the wasm module and calls small exported functions that return plain data structures like bytes or basic values. See [WASM Boundary](./wasm-boundary.md).
