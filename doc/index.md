# GBA WASM

A WebAssembly-based Game Boy Advance emulator and browser playground built for responsive, modern web delivery.

## Why this project exists

This project brings together a browser UI and a high-performance Rust emulator core. The goal is to make GBA emulation available in a web environment without sacrificing the correctness, determinism, and performance benefits of native-style emulation logic.

## Project principles

- Rust for the emulator core and memory model
- WebAssembly for browser execution
- React + Vite for a responsive app shell
- GitHub Pages for project documentation
- Test-driven milestone development for CPU, GPU, and memory features

## Current milestone

The first milestone focuses on the core GBA memory map and GPU framebuffer layer:

- SRAM and working memory layout
- GBA-style memory address mapping
- GPU framebuffer representation
- pixel drawing and test pattern generation
- Rust unit coverage for the core model

## Documentation layout

This site is organized into three sections:

1. Getting Started: how the project is bootstrapped and developed
2. Architecture: how the emulator core, browser glue, and WebAssembly boundary fit together
3. Rust Core: memory, GPU, and testing details for the first implementation milestone

## Quick links

- Setup and workflow
- Memory model
- GPU pipeline
- Build and test commands
