export type GbaWasmModule = {
  init: () => void
  hello: (name: string) => string
  inspect_rom: (rom: Uint8Array) => string
  run_rom: (rom: Uint8Array, steps: number) => Uint8ClampedArray
  run_thumb_rom: (rom: Uint8Array, steps: number) => Uint8ClampedArray
  create_emulator: (rom: Uint8Array) => GbaWasmEmulator
}

export type GbaWasmEmulator = {
  step_frame: () => Uint8ClampedArray
  free: () => void
}

type GeneratedWasmEmulator = {
  step_frame: () => Uint8Array
  free: () => void
}

type GeneratedWasmModule = {
  default: () => Promise<WebAssembly.Exports>
  init: () => void
  hello: (name: string) => string
  inspect_rom: (rom: Uint8Array) => string
  run_rom: (rom: Uint8Array, steps: number) => Uint8Array
  run_thumb_rom: (rom: Uint8Array, steps: number) => Uint8Array
  WasmEmulator: new (rom: Uint8Array) => GeneratedWasmEmulator
}

let wasmModule: GbaWasmModule | null = null

export async function loadGbaWasm(): Promise<GbaWasmModule> {
  if (wasmModule) {
    return wasmModule
  }

  const moduleUrl = new URL(
    `${import.meta.env.BASE_URL}wasm/gba_wasm.js`,
    window.location.origin,
  ).href
  const mod = (await import(/* @vite-ignore */ moduleUrl)) as GeneratedWasmModule
  await mod.default()

  const api: GbaWasmModule = {
    init: mod.init,
    hello: mod.hello,
    inspect_rom: mod.inspect_rom,
    run_rom: (rom: Uint8Array, steps: number) =>
      new Uint8ClampedArray(mod.run_rom(rom, steps)),
    run_thumb_rom: (rom: Uint8Array, steps: number) =>
      new Uint8ClampedArray(mod.run_thumb_rom(rom, steps)),
    create_emulator: (rom: Uint8Array) => {
      const emulator = new mod.WasmEmulator(rom)
      return {
        step_frame: () => new Uint8ClampedArray(emulator.step_frame()),
        free: () => emulator.free(),
      }
    },
  }

  wasmModule = api
  api.init()
  return api
}
