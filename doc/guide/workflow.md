# Development Workflow

## Requirements

- Rust with the `wasm32-unknown-unknown` target and [`wasm-pack`](https://rustwasm.github.io/wasm-pack/)
- Node.js and pnpm (the version is pinned in `package.json`)

## Commands

| Command | Purpose |
| --- | --- |
| `pnpm dev` | Build the WASM core into `public/wasm/`, then start the Vite dev server |
| `pnpm build` | Build the WASM core, type-check, and produce the production app |
| `pnpm wasm:build` | Rebuild only the WASM core (`wasm-pack build ./rust --target web --release`) |
| `pnpm test` | Run the frontend tests with Vitest |
| `pnpm lint` | Run ESLint |
| `cargo test` (in `rust/`) | Run the Rust unit and integration tests |
| `pnpm docs:dev` | Serve this documentation locally with VitePress |
| `pnpm docs:build` | Build the documentation |
| `pnpm boxart:fetch` | Download the featured-cartridge covers into `public/boxart/` as WebP |

## Standard loop

1. Add or update a focused test in the Rust crate
2. Implement the corresponding subsystem in the emulator core
3. Run `cargo test` for the affected module, or `pnpm test` for browser code
4. Rebuild the WASM output with `pnpm wasm:build` when the core changes
5. Validate the app in the Vite dev environment

See [Testing Strategy](../rust-core/testing-strategy.md) for what the tests cover and the fuzzing options.

## Browser shell

The React app is responsible for UI, canvas rendering, and browser integration. It should not contain emulator logic beyond the minimal bridge and event handling required to drive the core.

## Documentation

The docs site lives under the `doc/` folder and is built with VitePress. `.github/workflows/deploy-docs.yml` publishes it to GitHub Pages under `/gba-wasm/docs/`.

User Manual screenshots are WebP files in `doc/manual/images/`. Take them from the app running the MIT-licensed test ROMs in `rust/tests/fixtures/gba-tests/`, never from commercial games.
