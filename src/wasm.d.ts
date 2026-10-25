declare module '/wasm/*' {
  export function init(): void
  export function hello(name: string): string
  export function make_test_frame(width: number, height: number): Uint8Array
}
