import { useEffect, useRef } from 'react'
import { GBA_KEYS, dpadMaskForVector } from './keypad'

const DPAD = 'dpad'

/** Returns the buttons pressed by a touch at the given viewport point. */
function maskAtPoint(x: number, y: number): number {
  const target = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-gba]')
  if (!target) return 0
  const key = target.dataset.gba
  if (key === DPAD) {
    const rect = target.getBoundingClientRect()
    const radius = rect.width / 2
    return dpadMaskForVector((x - rect.left - radius) / radius, (y - rect.top - rect.height / 2) / radius)
  }
  return GBA_KEYS[key as keyof typeof GBA_KEYS] ?? 0
}

/**
 * On-screen GBA buttons. Every pointer is tracked separately and re-hit-tested on move, so a
 * finger can slide between buttons (or across D-pad directions) without lifting.
 */
export function TouchControls({ onChange }: { onChange: (pressed: number) => void }) {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const onChangeRef = useRef(onChange)
  useEffect(() => { onChangeRef.current = onChange }, [onChange])

  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    const pointers = new Map<number, number>()
    let reported = 0
    const report = () => {
      const mask = [...pointers.values()].reduce((combined, bit) => combined | bit, 0)
      if (mask === reported) return
      if (mask & ~reported) navigator.vibrate?.(12)
      reported = mask
      root.querySelectorAll<HTMLElement>('[data-gba]').forEach((element) => {
        const key = element.dataset.gba
        const bits = key === DPAD ? GBA_KEYS.UP | GBA_KEYS.DOWN | GBA_KEYS.LEFT | GBA_KEYS.RIGHT : GBA_KEYS[key as keyof typeof GBA_KEYS]
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
      if (event.type === 'pointerdown') root.setPointerCapture(event.pointerId)
      else if (!pointers.has(event.pointerId)) return
      event.preventDefault()
      pointers.set(event.pointerId, maskAtPoint(event.clientX, event.clientY))
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
    <div className="touch-controls" ref={rootRef} aria-hidden="true">
      <div className="touch-shoulders"><span className="touch-shoulder" data-gba="L">L</span><span className="touch-shoulder" data-gba="R">R</span></div>
      <div className="touch-main">
        <div className="touch-dpad" data-gba={DPAD}><i className="touch-dpad-h" /><i className="touch-dpad-v" /></div>
        <div className="touch-face"><span className="touch-face-button touch-b" data-gba="B">B</span><span className="touch-face-button touch-a" data-gba="A">A</span></div>
      </div>
      <div className="touch-system"><span className="touch-pill" data-gba="SELECT">SELECT</span><span className="touch-pill" data-gba="START">START</span></div>
    </div>
  )
}
