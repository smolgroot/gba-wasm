export type GbaWasmModule = {
  init: () => void
  hello: (name: string) => string
  make_test_frame: (width: number, height: number) => Uint8ClampedArray
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
  const mod = (await import(/* @vite-ignore */ moduleUrl)) as any
  await mod.default()

  const api: GbaWasmModule = {
    init: mod.init,
    hello: mod.hello,
    make_test_frame: (width: number, height: number) =>
      new Uint8ClampedArray(mod.make_test_frame(width, height)),
  }

  wasmModule = api
  api.init()
  return api
}
