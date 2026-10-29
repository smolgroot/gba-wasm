/** Colour themes for the on-screen buttons; the palettes live in App.css under `[data-touch-theme]`. */
export const TOUCH_THEMES = [
  { id: 'gba-wasm', label: 'GBA-WASM' },
  { id: 'indigo', label: 'Indigo GBA' },
  { id: 'pikachu', label: 'Electric Pikachu' },
  { id: 'emerald', label: 'Emerald' },
  { id: 'fire-red', label: 'Fire Red' },
  { id: 'hyrule', label: 'Hyrule' },
] as const

export type TouchTheme = (typeof TOUCH_THEMES)[number]['id']

export function isTouchTheme(value: unknown): value is TouchTheme {
  return TOUCH_THEMES.some((theme) => theme.id === value)
}
