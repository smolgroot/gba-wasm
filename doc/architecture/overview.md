# Architecture Overview

The project is split into a few clear concerns:

- `rust/`: emulator core logic, memory model, GPU, APU, saves, and test suite
- `src/`: browser application code and wasm bridge
- `public/`: generated wasm assets (`public/wasm/`), featured cover art, and app icons
- `doc/`: public project documentation built with VitePress
- `scripts/`: maintenance scripts, such as fetching cover art

## System at a glance

Pecten is a single-page React app that runs the emulator as a WebAssembly module in the same browser tab. The React shell owns everything the player sees and touches; the Rust core owns the machine. They meet only at the `WasmEmulator` API ([WASM Boundary](./wasm-boundary.md)).

```mermaid
flowchart LR
  subgraph Browser["Browser tab"]
    direction TB
    subgraph Shell["Vite + React shell (src/)"]
      direction TB
      App["App.tsx<br/>routes, settings, frame loop"]
      Input["keypad.ts · TouchControls.tsx<br/>haptics.ts"]
      Home["CoverFlow.tsx · featuredGames.ts<br/>InstallPrompt.tsx"]
      Audio["audio.ts<br/>AudioWorklet + GainNode"]
      Store["backupStorage.ts · savedGames.ts"]
      Bridge["wasmBridge.ts<br/>typed bindings"]
    end
    subgraph Wasm["WebAssembly (public/wasm/)"]
      direction TB
      Glue["pecten.js<br/>wasm-bindgen glue"]
      Core["pecten_bg.wasm<br/>Rust core: WasmEmulator → GbaCore"]
    end
    Canvas[("canvas 240×160")]
    IDB[("IndexedDB<br/>ROM library, saves, states")]
    LS[("localStorage<br/>settings")]
    SW["Service worker<br/>(vite-plugin-pwa, offline)"]
  end

  Input -- "key mask" --> App
  Home --> App
  App <--> Bridge
  Bridge <--> Glue
  Glue <--> Core
  App -- "putImageData" --> Canvas
  App -- "i16 stereo samples" --> Audio
  App <--> Store
  Store <--> IDB
  App <--> LS
```

## Build pipeline

Two toolchains produce one static site. `wasm-pack` compiles the Rust crate to WebAssembly and writes the module and its JavaScript glue into `public/wasm/`. Vite then builds the React app; it copies `public/` as-is, and `wasmBridge.ts` imports `wasm/pecten.js` at runtime (`/* @vite-ignore */`), so the wasm module is never bundled. VitePress builds this documentation separately, and the deploy workflow places it under `/docs/`. The commands are listed in [Development Workflow](../guide/workflow.md).

```mermaid
flowchart LR
  subgraph Sources
    Rust["rust/<br/>Cargo crate (cdylib)"]
    TS["src/ · index.html<br/>React 19 + TypeScript + MUI"]
    Pub["public/<br/>icons, boxart, logo"]
    Docs["doc/<br/>Markdown"]
  end

  Rust -- "pnpm wasm:build<br/>wasm-pack --target web --release" --> Out["public/wasm/<br/>pecten.js · pecten_bg.wasm · pecten.d.ts"]
  Out --> Pub
  TS -- "tsc -b<br/>(type-check)" --> Vite
  Pub --> Vite["vite build<br/>@vitejs/plugin-react<br/>vite-plugin-pwa"]
  Vite --> Dist["dist/<br/>app, service worker, manifest"]
  Docs -- "pnpm docs:build<br/>VitePress" --> DocsDist["doc/.vitepress/dist/"]
  DocsDist -- "copied" --> DistDocs["dist/docs/"]
  Dist --> Pages
  DistDocs --> Pages["GitHub Pages<br/>pecten.app"]
```

`pnpm build` runs the first three steps in order (`wasm:build`, `tsc -b`, `vite build`); `pnpm dev` runs `wasm:build` and then the Vite dev server, so a core change needs `pnpm wasm:build` (or a restart) to reach the page. On every push to `main`, `.github/workflows/deploy-docs.yml` installs Rust with the `wasm32-unknown-unknown` target and `wasm-pack`, runs `pnpm build` and `pnpm docs:build`, and publishes `dist/`.

## Inside the core

`GbaCore` owns three parts: the CPU, the memory map, and the renderer. `MemoryMap` owns every memory region and every device that is clocked with memory, so the CPU advances all of them through one call, `advance_video_cycles`. The browser never reaches past `WasmEmulator`.

```mermaid
flowchart TB
  JS(["JavaScript (wasmBridge.ts)"]) --> WE["WasmEmulator<br/>lib.rs · #[wasm_bindgen]"]
  WE --> Core["GbaCore<br/>step_frame · save_state / load_state"]
  Core --> Intro["BootIntro<br/>boot_intro.rs · boot_logo.rs<br/>(optional, before the cartridge)"]
  Core --> CPU["ThumbCpu<br/>cpu.rs<br/>ARM + Thumb interpreter, HLE SWI services"]
  Core --> GPU["GpuCore<br/>gpu.rs<br/>scanline renderer → RGBA framebuffer"]
  Core --> Mem["MemoryMap<br/>memory.rs<br/>bus, I/O registers, video clock"]
  CPU -- "read / write<br/>advance_video_cycles" --> Mem
  GPU -. "reads VRAM, palette,<br/>OAM, display I/O" .-> Mem

  subgraph Owned["Owned by MemoryMap"]
    direction LR
    BIOS["HLE BIOS image<br/>bios.rs (16 KiB)"]
    RAM["EWRAM 256 KiB<br/>IWRAM 32 KiB"]
    Video["VRAM 96 KiB<br/>palette · OAM"]
    IO["I/O registers<br/>IE · IF · IME · keypad"]
    Timers["4 timers"]
    DMA["4 DMA channels"]
    APU["Apu<br/>apu.rs"]
    Cart["Cartridge ROM<br/>cartridge.rs"]
    Backup["Backup<br/>backup.rs<br/>SRAM · Flash · EEPROM"]
  end
  Mem --- Owned
  State["state.rs<br/>impl_state! snapshot"] -. "serializes" .-> Core
```

## CPU, HLE BIOS, and interrupts

Each call to `ThumbCpu::step_with_cycles` does exactly one unit of work and returns the cycles it took, which `MemoryMap` then spends on timers, the APU, display timing, and DMA triggers. `GbaCore::step_frame` repeats it until the display enters VBlank, rendering each scanline as soon as its HBlank begins ([GPU Pipeline](./gpu-pipeline.md)).

```mermaid
flowchart TB
  Start(["step_with_cycles"]) --> DMA{"DMA pending?"}
  DMA -- yes --> RunDMA["run DMA transfer"] --> Adv
  DMA -- no --> Halt{"halted<br/>(Halt / IntrWait)?"}
  Halt -- "yes, no wake-up" --> Skip["skip to next display<br/>or timer event"] --> Adv
  Halt -- "no, or IE & IF ≠ 0" --> IRQ{"IRQ line asserted<br/>and CPSR.I clear?"}
  IRQ -- yes --> Enter["enter IRQ mode<br/>PC = 0x18 (BIOS vector)"] --> Adv
  IRQ -- no --> Idle{"confirmed idle loop?"}
  Idle -- yes --> Skip
  Idle -- no --> Exec["decode and execute<br/>one ARM or Thumb instruction"]
  Exec --> SWI{"SWI?"}
  SWI -- "yes" --> HLE["HLE service in Rust<br/>Div, CpuSet, LZ77, IntrWait, …"] --> Adv
  SWI -- no --> Adv["MemoryMap::advance_video_cycles<br/>timers → APU → display IRQs → DMA triggers"]
  Adv --> Done(["return cycles"])
```

No real BIOS runs. The generated image only holds the exception vectors and the hardware IRQ dispatcher, so a game's interrupt handler is entered exactly as on hardware, while `SWI` calls are answered by Rust code in a single step ([HLE BIOS](../rust-core/bios.md)):

```mermaid
sequenceDiagram
  autonumber
  participant Game as Game code (ROM / IWRAM)
  participant CPU as ThumbCpu
  participant BIOS as HLE BIOS image (0x0000–0x3FFF)
  participant Handler as Game IRQ handler
  participant Mem as MemoryMap (I/O)

  Note over Game,CPU: Software interrupt
  Game->>CPU: SWI n
  CPU->>CPU: execute_bios_call(n) in Rust<br/>(no Supervisor mode entry)
  CPU-->>Game: results in r0–r3, next instruction

  Note over Game,Mem: Hardware interrupt
  Mem-->>CPU: IE & IF ≠ 0, IME = 1
  CPU->>BIOS: enter IRQ mode, jump to vector 0x18
  BIOS->>BIOS: B dispatcher (0x128)<br/>STMFD sp!, {r0-r3, r12, lr}
  BIOS->>Handler: LDR pc, [0x03007FFC]
  Handler->>Mem: acknowledge IF<br/>(and BIOS flags at 0x03007FF8)
  Handler-->>BIOS: BX lr
  BIOS-->>Game: LDMFD sp!, {r0-r3, r12, lr}<br/>SUBS pc, lr, #4
```

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
