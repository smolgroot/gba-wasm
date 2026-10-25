# Architecture Overview

The project is split into a few clear concerns:

- `rust/`: emulator core logic, memory model, GPU, and test suite
- `src/`: browser application code and wasm bridge
- `public/`: generated wasm assets and runtime app resources
- `doc/`: public project documentation built with VitePress

## Separation of responsibilities

### Emulator core

The Rust crate owns simulation correctness: reading and writing memory, processing rendering state, and modeling game-system behavior.

### Browser layer

The app shell owns user experience: controls, canvas presentation, input handling, and the browser-specific environment.

### WASM boundary

The boundary between the browser and the Rust core is intentionally narrow. The app loads the wasm module and calls small exported functions that return plain data structures like bytes or basic values.
