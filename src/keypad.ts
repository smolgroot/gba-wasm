// Keyboard mapping to the GBA KEYINPUT bits (1 = pressed in the mask sent to the core).

export const GBA_KEYS = {
  A: 1 << 0,
  B: 1 << 1,
  SELECT: 1 << 2,
  START: 1 << 3,
  RIGHT: 1 << 4,
  LEFT: 1 << 5,
  UP: 1 << 6,
  DOWN: 1 << 7,
  R: 1 << 8,
  L: 1 << 9,
} as const

/** Keyed by `KeyboardEvent.code`, so the layout is position-based (QWERTY labels). */
const KEYBOARD_MAPPING: Record<string, number> = {
  KeyX: GBA_KEYS.A,
  KeyZ: GBA_KEYS.B,
  ShiftLeft: GBA_KEYS.SELECT,
  ShiftRight: GBA_KEYS.SELECT,
  Enter: GBA_KEYS.START,
  ArrowRight: GBA_KEYS.RIGHT,
  ArrowLeft: GBA_KEYS.LEFT,
  ArrowUp: GBA_KEYS.UP,
  ArrowDown: GBA_KEYS.DOWN,
  KeyS: GBA_KEYS.R,
  KeyA: GBA_KEYS.L,
}

export function gbaKeyForCode(code: string): number | undefined {
  return KEYBOARD_MAPPING[code]
}

/**
 * Tracks pressed keyboard keys and reports the combined button mask. Each physical key is
 * tracked separately, so releasing one Shift key keeps Select held while the other is down.
 */
export function attachKeyboard(target: Window, onChange: (pressed: number) => void) {
  const held = new Map<string, number>()
  const report = () => onChange([...held.values()].reduce((mask, bit) => mask | bit, 0))

  const onKeyDown = (event: KeyboardEvent) => {
    const bit = gbaKeyForCode(event.code)
    if (bit === undefined || event.ctrlKey || event.metaKey || event.altKey) return
    if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return
    event.preventDefault()
    if (!held.has(event.code)) {
      held.set(event.code, bit)
      report()
    }
  }
  const onKeyUp = (event: KeyboardEvent) => {
    if (held.delete(event.code)) report()
  }
  // Key-up events are lost when focus leaves the page, so release everything.
  const releaseAll = () => {
    if (held.size === 0) return
    held.clear()
    report()
  }

  target.addEventListener('keydown', onKeyDown)
  target.addEventListener('keyup', onKeyUp)
  target.addEventListener('blur', releaseAll)
  document.addEventListener('visibilitychange', releaseAll)
  return () => {
    target.removeEventListener('keydown', onKeyDown)
    target.removeEventListener('keyup', onKeyUp)
    target.removeEventListener('blur', releaseAll)
    document.removeEventListener('visibilitychange', releaseAll)
  }
}

// tan(22.5°): each D-pad direction covers a 45° sector, so diagonals are as easy to hit as cardinals.
const DPAD_SECTOR_SLOPE = Math.tan(Math.PI / 8)

/**
 * Maps a touch offset from the D-pad centre (normalised so the pad radius is 1, y pointing down)
 * to the pressed direction bits. Touches inside the dead zone press nothing.
 */
export function dpadMaskForVector(dx: number, dy: number, deadZone = 0.2): number {
  if (Math.hypot(dx, dy) < deadZone) return 0
  let mask = 0
  if (Math.abs(dx) > DPAD_SECTOR_SLOPE * Math.abs(dy)) mask |= dx > 0 ? GBA_KEYS.RIGHT : GBA_KEYS.LEFT
  if (Math.abs(dy) > DPAD_SECTOR_SLOPE * Math.abs(dx)) mask |= dy > 0 ? GBA_KEYS.DOWN : GBA_KEYS.UP
  return mask
}
