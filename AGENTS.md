# AGENTS.md

This repository is for a Game Boy Advance / WebAssembly project. Keep the emulator core, browser glue, and build tooling clearly separated so changes stay easy to reason about and test.

## Working conventions

- Prefer small, targeted changes over broad refactors.
- Before making changes, inspect the actual project configuration and entry points (`package.json`, build scripts, CMake/Makefiles, Emscripten config, or other toolchain files that are present in the repo).
- Follow the repository’s existing toolchain and naming conventions rather than introducing a new framework or build pattern unless the task explicitly requires it.
- Keep platform-specific code isolated: browser-facing code, WebAssembly glue, and emulator core logic should have clear boundaries.
- Preserve emulator correctness, especially CPU, timing, memory, and input semantics. Avoid “reasonable-looking” shortcuts that change behavior.
- Favor explicit state and deterministic data flow over hidden global state.
- If a change affects behavior, add or update focused tests in the same area.

## Recommended workflow

1. Read the most relevant build/test configuration and any existing docs before editing.
2. Make the smallest possible fix or feature change.
3. Validate with the repo’s existing commands for the affected area.
4. Keep documentation concise and link to higher-level docs rather than duplicating them.

## Documentation guidance

- Do not duplicate existing project documentation. Prefer links to docs in the repo when they already explain architecture or build steps.
- If the project grows, add specialized agent instructions for specific areas (for example: emulator core, WebAssembly bindings, browser UI, or tests) instead of cramming everything into one file.

## Practical guardrails

- Do not assume a package manager or toolchain without checking the repo.
- Do not rewrite surrounding code just to make a local patch fit a preferred style.
- Prefer reproducible build commands and explicit configuration over ad hoc scripts.
- When in doubt, check the project’s actual files and conventions before running commands or introducing new dependencies.
