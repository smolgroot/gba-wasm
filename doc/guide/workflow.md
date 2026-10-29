# Development Workflow

## Standard loop

1. Add or update a focused test in the Rust crate
2. Implement the corresponding subsystem in the emulator core
3. Run `cargo test` for the affected module, or `pnpm test` for browser code
4. Build the wasm output for the browser bridge
5. Validate the app in the Vite dev environment

## Browser shell

The React app is responsible for UI, canvas rendering, and browser integration. It should not contain emulator logic beyond the minimal bridge and event handling required to drive the core.

## Documentation

The docs site lives under the `doc/` folder and is built with VitePress. It can be deployed to GitHub Pages with a static site configuration that preserves versioned release docs or a single docs root.
