export type GbaWasmModule = {
  init: () => void
  hello: (name: string) => string
  inspect_rom: (rom: Uint8Array) => string
  run_rom: (rom: Uint8Array, steps: number) => Uint8ClampedArray
  run_thumb_rom: (rom: Uint8Array, steps: number) => Uint8ClampedArray
}

type GeneratedWasmModule = {
  default: () => Promise<WebAssembly.Exports>
  init: () => void
  hello: (name: string) => string
  inspect_rom: (rom: Uint8Array) => string
  run_rom: (rom: Uint8Array, steps: number) => Uint8Array
  run_thumb_rom: (rom: Uint8Array, steps: number) => Uint8Array
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
  }

  wasmModule = api
  api.init()
  return api
}
