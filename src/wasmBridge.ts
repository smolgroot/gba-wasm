export type GbaWasmModule = {
  init: () => void
  hello: (name: string) => string
  render_mode3: (vram: Uint8Array) => Uint8ClampedArray
}

type GeneratedWasmModule = {
  default: () => Promise<WebAssembly.Exports>
  init: () => void
  hello: (name: string) => string
  render_mode3: (vram: Uint8Array) => Uint8Array
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
    render_mode3: (vram: Uint8Array) =>
      new Uint8ClampedArray(mod.render_mode3(vram)),
  }

  wasmModule = api
  api.init()
  return api
}
