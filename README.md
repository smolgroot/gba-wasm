<div align="center">
  <img src="public/logo.png" alt="logo" width="320" />

  <h1>GBA WASM</h1>

  <p>
    Game Boy Advance emulator and browser playground built with Rust and WebAssembly.
  </p>

  <p>
    <!-- <a href="https://github.com/smolgroot/gba-wasm/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/smolgroot/gba-wasm/ci.yml?branch=master&label=CI&color=4e6a5c&labelColor=4e6a5c&logoColor=white" /></a>
    <a href="https://github.com/smolgroot/gba-wasm/releases"><img alt="release" src="https://img.shields.io/github/v/release/smolgroot/gba-wasm?include_prereleases&label=release&color=4e6a5c&labelColor=4e6a5c&logoColor=white" /></a>
    <a href="https://github.com/smolgroot/gba-wasm/pkgs/container/gba-wasm"><img alt="container image" src="https://img.shields.io/badge/ghcr.io-gba--wasm-4e6a5c?labelColor=4e6a5c&logoColor=white" /></a> -->
    <a href="#license"><img alt="license" src="https://img.shields.io/badge/license-MIT-550055?labelColor=550055&logoColor=white" /></a>
  </p>
</div>

<hr />

> **Note:** This project is in active development. The CPU passes the jsmolka/gba-tests ARM, Thumb, and memory suites, and the core renders all display modes, plays audio, and emulates cartridge saves, but accuracy varies by game. See [TASKS.md](TASKS.md) for what is missing.

## Documentation

The [documentation](https://smolgroot.github.io/gba-wasm/docs/) (source in [`doc/`](doc/)) includes a **User Manual** (saves, settings, mobile and desktop play, controls) and the architecture of the Rust core.

```sh
pnpm install
pnpm dev        # build the WASM core and start the app
pnpm docs:dev   # serve the documentation
```