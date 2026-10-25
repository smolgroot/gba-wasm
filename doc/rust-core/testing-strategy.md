# Testing Strategy

The Rust core follows a small, explicit testing strategy tuned for emulator work.

## Principles

- verify each subsystem with a narrow unit test
- keep tests deterministic and pure
- prefer byte-accurate checks over broad assertions
- validate rendering state before integrating with the browser

## Current coverage

The milestone includes tests for:

- memory map read/write semantics
- GPU pixel drawing within a framebuffer

## Recommended next additions

- CPU instruction decode tests
- memory timing tests
- sprite and background object behavior
- framebuffer comparison snapshots for known patterns
