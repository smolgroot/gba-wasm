import { useEffect, useRef } from 'react'
import { GBA_KEYS, dpadMaskForVector } from './keypad'
import type { TouchTheme } from './touchThemes'
import { hapticTick, type HapticStrength } from './haptics'

const DPAD = 'dpad'
const DPAD_DIRECTIONS = GBA_KEYS.UP | GBA_KEYS.DOWN | GBA_KEYS.LEFT | GBA_KEYS.RIGHT

/** Centre and radius of the D-pad, captured when a finger grabs it so its tilt cannot shift the origin. */
type Joystick = { x: number; y: number; radius: number }
type TrackedPointer = { mask: number; joystick: Joystick | null }

/**
 * Returns the button under a viewport point. The D-pad has a capture zone around it (see
 * `.touch-dpad::before`) that may overlap other buttons, so any real button there wins.
 */
function buttonAtPoint(x: number, y: number): HTMLElement | null {
  let dpad: HTMLElement | null = null
  for (const element of document.elementsFromPoint(x, y)) {
    const target = element.closest<HTMLElement>('[data-gba]')
    if (!target) continue
    if (target.dataset.gba !== DPAD) return target
    dpad ??= target
  }
  return dpad
}

function joystickFor(dpad: HTMLElement): Joystick {
  const rect = dpad.getBoundingClientRect()
  return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2, radius: rect.width / 2 }
}

/**
 * Updates a pointer's pressed buttons. Once a finger reaches the D-pad it acts as a joystick for
 * the rest of the touch: the direction follows the finger's angle from the pad's centre, even
 * when it slides past the pad's edge.
 */
function updatePointer(pointer: TrackedPointer, x: number, y: number) {
  if (!pointer.joystick) {
    const target = buttonAtPoint(x, y)
    if (target?.dataset.gba === DPAD) pointer.joystick = joystickFor(target)
    else {
      pointer.mask = GBA_KEYS[target?.dataset.gba as keyof typeof GBA_KEYS] ?? 0
      return
    }
  }
  const { joystick } = pointer
  pointer.mask = dpadMaskForVector((x - joystick.x) / joystick.radius, (y - joystick.y) / joystick.radius)
}

/**
 * On-screen GBA buttons. Every pointer is tracked separately and re-hit-tested on move, so a
 * finger can slide between buttons without lifting, and steer the D-pad like a joystick.
 */
export function TouchControls({ theme, haptics, onChange }: { theme: TouchTheme; haptics: HapticStrength; onChange: (pressed: number) => void }) {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const onChangeRef = useRef(onChange)
  const hapticsRef = useRef(haptics)
  useEffect(() => { onChangeRef.current = onChange }, [onChange])
  useEffect(() => { hapticsRef.current = haptics }, [haptics])

  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    const pointers = new Map<number, TrackedPointer>()
    let reported = 0
    const report = () => {
      const mask = [...pointers.values()].reduce((combined, pointer) => combined | pointer.mask, 0)
      if (mask === reported) return
      const pressed = mask & ~reported
      // Tick for newly pressed buttons only: D-pad directions change too often while steering.
      const strength = hapticsRef.current
      if ((pressed & ~DPAD_DIRECTIONS) !== 0 && strength !== 'off') hapticTick(strength)
      reported = mask
      root.querySelectorAll<HTMLElement>('[data-gba]').forEach((element) => {
        const key = element.dataset.gba
        const bits = key === DPAD ? DPAD_DIRECTIONS : GBA_KEYS[key as keyof typeof GBA_KEYS]
        element.classList.toggle('is-pressed', (mask & bits) !== 0)
        // Tilt the cross towards the held direction(s), like a physical rocker.
        if (key === DPAD) {
          const tiltX = (mask & GBA_KEYS.UP ? 10 : 0) - (mask & GBA_KEYS.DOWN ? 10 : 0)
          const tiltY = (mask & GBA_KEYS.RIGHT ? 10 : 0) - (mask & GBA_KEYS.LEFT ? 10 : 0)
          element.style.transform = `perspective(320px) rotateX(${tiltX}deg) rotateY(${tiltY}deg)`
        }
      })
      onChangeRef.current(mask)
    }
    const track = (event: PointerEvent) => {
      let pointer = pointers.get(event.pointerId)
      if (event.type === 'pointerdown') {
        root.setPointerCapture(event.pointerId)
        pointer = { mask: 0, joystick: null }
        pointers.set(event.pointerId, pointer)
      } else if (!pointer) return
      event.preventDefault()
      updatePointer(pointer, event.clientX, event.clientY)
      report()
    }
    const release = (event: PointerEvent) => {
      if (pointers.delete(event.pointerId)) report()
    }
    const releaseAll = () => {
      pointers.clear()
      report()
    }
    const preventDefault = (event: Event) => event.preventDefault()

    root.addEventListener('pointerdown', track)
    root.addEventListener('pointermove', track)
    root.addEventListener('pointerup', release)
    root.addEventListener('pointercancel', release)
    root.addEventListener('contextmenu', preventDefault)
    window.addEventListener('blur', releaseAll)
    document.addEventListener('visibilitychange', releaseAll)
    return () => {
      root.removeEventListener('pointerdown', track)
      root.removeEventListener('pointermove', track)
      root.removeEventListener('pointerup', release)
      root.removeEventListener('pointercancel', release)
      root.removeEventListener('contextmenu', preventDefault)
      window.removeEventListener('blur', releaseAll)
      document.removeEventListener('visibilitychange', releaseAll)
      pointers.clear()
      onChangeRef.current(0)
    }
  }, [])

  return (
    <div className="touch-controls" ref={rootRef} data-touch-theme={theme} aria-hidden="true">
      <div className="touch-shoulders"><span className="touch-shoulder" data-gba="L">L</span><span className="touch-shoulder" data-gba="R">R</span></div>
      <div className="touch-main">
        <div className="touch-dpad" data-gba={DPAD}><i className="touch-dpad-h" /><i className="touch-dpad-v" /></div>
        <div className="touch-face"><span className="touch-face-button touch-b" data-gba="B">B</span><span className="touch-face-button touch-a" data-gba="A">A</span></div>
      </div>
      <div className="touch-system"><span className="touch-pill" data-gba="SELECT">SELECT</span><span className="touch-pill" data-gba="START">START</span></div>
    </div>
  )
}
