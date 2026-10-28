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
  backup_type: () => string
  backup_data: () => Uint8Array
  load_backup: (data: Uint8Array) => void
  take_backup_dirty: () => boolean
  set_pressed_keys: (pressed: number) => void
  take_audio_samples: () => Int16Array
  audio_sample_rate: () => number
  free: () => void
}

type GeneratedWasmEmulator = Omit<GbaWasmEmulator, 'step_frame'> & {
  step_frame: () => Uint8Array
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
        backup_type: () => emulator.backup_type(),
        backup_data: () => emulator.backup_data(),
        load_backup: (data: Uint8Array) => emulator.load_backup(data),
        take_backup_dirty: () => emulator.take_backup_dirty(),
        set_pressed_keys: (pressed: number) => emulator.set_pressed_keys(pressed),
        take_audio_samples: () => emulator.take_audio_samples(),
        audio_sample_rate: () => emulator.audio_sample_rate(),
        free: () => emulator.free(),
      }
    },
  }

  wasmModule = api
  api.init()
  return api
}
