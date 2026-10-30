import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { GBA_KEYS } from './keypad'
import { TouchControls } from './TouchControls'
import type { HapticStrength } from './haptics'

vi.mock('./haptics', () => ({ hapticTick: vi.fn() }))
const { hapticTick } = await import('./haptics')

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true

type Box = { left: number; top: number; width: number; height: number }

// A fixed layout, since jsdom does no layout or hit testing. The D-pad (centre 50,150,
// radius 50) has a 20 px capture zone, like `.touch-dpad::before`, that overlaps SELECT.
const LAYOUT: Record<string, Box> = {
  L: { left: 0, top: 0, width: 60, height: 30 },
  R: { left: 300, top: 0, width: 60, height: 30 },
  dpad: { left: 0, top: 100, width: 100, height: 100 },
  B: { left: 240, top: 140, width: 40, height: 40 },
  A: { left: 300, top: 120, width: 40, height: 40 },
  SELECT: { left: 90, top: 190, width: 40, height: 20 },
  START: { left: 160, top: 190, width: 40, height: 20 },
}
const DPAD_CAPTURE = 20

function contains({ left, top, width, height }: Box, x: number, y: number, margin = 0) {
  return x >= left - margin && x < left + width + margin && y >= top - margin && y < top + height + margin
}

function center(name: string) {
  const { left, top, width, height } = LAYOUT[name]
  return { x: left + width / 2, y: top + height / 2 }
}

let container: HTMLDivElement
let root: Root
let onChange: ReturnType<typeof vi.fn<(pressed: number) => void>>

beforeEach(() => {
  vi.mocked(hapticTick).mockClear()
  onChange = vi.fn()
  container = document.body.appendChild(document.createElement('div'))
  root = createRoot(container)
  document.elementsFromPoint = (x: number, y: number) =>
    [...container.querySelectorAll<HTMLElement>('[data-gba]')].filter((element) => {
      const name = element.dataset.gba!
      return contains(LAYOUT[name], x, y, name === 'dpad' ? DPAD_CAPTURE : 0)
    })
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const { left, top, width, height } = LAYOUT[this.dataset.gba ?? ''] ?? { left: 0, top: 0, width: 0, height: 0 }
    return DOMRect.fromRect({ x: left, y: top, width, height })
  })
  HTMLElement.prototype.setPointerCapture = vi.fn()
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.restoreAllMocks()
})

function render(haptics: HapticStrength = 'off') {
  act(() => root.render(<TouchControls theme="gba-wasm" haptics={haptics} onChange={onChange} />))
  return container.querySelector<HTMLElement>('.touch-controls')!
}

function pointer(type: string, pointerId: number, point: { x: number; y: number }) {
  const event = new PointerEvent(type, { pointerId, clientX: point.x, clientY: point.y, bubbles: true, cancelable: true })
  act(() => {
    container.querySelector('.touch-controls')!.dispatchEvent(event)
  })
  return event
}

const last = () => onChange.mock.lastCall?.[0]

describe('TouchControls', () => {
  it('presses and releases a button', () => {
    render()
    const down = pointer('pointerdown', 1, center('A'))
    expect(last()).toBe(GBA_KEYS.A)
    expect(down.defaultPrevented).toBe(true)
    expect(container.querySelector('[data-gba="A"]')!.classList.contains('is-pressed')).toBe(true)
    pointer('pointerup', 1, center('A'))
    expect(last()).toBe(0)
    expect(container.querySelector('[data-gba="A"]')!.classList.contains('is-pressed')).toBe(false)
  })

  it('combines simultaneous fingers and releases each separately', () => {
    render()
    pointer('pointerdown', 1, center('A'))
    pointer('pointerdown', 2, center('B'))
    pointer('pointerdown', 3, center('L'))
    expect(last()).toBe(GBA_KEYS.A | GBA_KEYS.B | GBA_KEYS.L)
    pointer('pointerup', 2, center('B'))
    expect(last()).toBe(GBA_KEYS.A | GBA_KEYS.L)
    pointer('pointerup', 1, center('A'))
    pointer('pointerup', 3, center('L'))
    expect(last()).toBe(0)
  })

  it('follows a finger sliding between buttons without lifting', () => {
    render()
    pointer('pointerdown', 1, center('B'))
    pointer('pointermove', 1, center('A'))
    expect(last()).toBe(GBA_KEYS.A)
    pointer('pointermove', 1, { x: 200, y: 100 })
    expect(last()).toBe(0)
    pointer('pointermove', 1, center('START'))
    expect(last()).toBe(GBA_KEYS.START)
  })

  it('ignores moves of pointers that never pressed, such as a hovering mouse', () => {
    render()
    pointer('pointermove', 7, center('A'))
    expect(onChange).not.toHaveBeenCalled()
  })

  it('prefers a real button over the D-pad capture zone', () => {
    render()
    pointer('pointerdown', 1, { x: 110, y: 200 })
    expect(last()).toBe(GBA_KEYS.SELECT)
  })

  it('steers the D-pad like a joystick, even past its edge', () => {
    render()
    const dpad = center('dpad')
    pointer('pointerdown', 1, { x: dpad.x + 30, y: dpad.y })
    expect(last()).toBe(GBA_KEYS.RIGHT)
    pointer('pointermove', 1, { x: dpad.x + 30, y: dpad.y - 30 })
    expect(last()).toBe(GBA_KEYS.UP | GBA_KEYS.RIGHT)
    // Far outside the pad, over the B button: still the D-pad, still a direction.
    pointer('pointermove', 1, { x: center('B').x, y: dpad.y })
    expect(last()).toBe(GBA_KEYS.RIGHT)
    // Back to the centre: the dead zone presses nothing.
    pointer('pointermove', 1, dpad)
    expect(last()).toBe(0)
    pointer('pointerup', 1, dpad)
    expect(last()).toBe(0)
  })

  it('releases a cancelled pointer', () => {
    render()
    pointer('pointerdown', 1, center('A'))
    pointer('pointerdown', 2, center('dpad'))
    pointer('pointermove', 2, { x: center('dpad').x, y: center('dpad').y + 40 })
    pointer('pointercancel', 1, center('A'))
    expect(last()).toBe(GBA_KEYS.DOWN)
    pointer('pointercancel', 2, center('dpad'))
    expect(last()).toBe(0)
  })

  it.each([
    ['the window loses focus', () => window.dispatchEvent(new Event('blur'))],
    ['the page is hidden', () => document.dispatchEvent(new Event('visibilitychange'))],
  ])('releases every button when %s', (_, loseFocus) => {
    render()
    pointer('pointerdown', 1, center('A'))
    pointer('pointerdown', 2, center('R'))
    act(loseFocus)
    expect(last()).toBe(0)
    // A move from a pointer released this way must not press again.
    pointer('pointermove', 1, center('A'))
    expect(last()).toBe(0)
  })

  it('reports a release when unmounted mid-press', () => {
    render()
    pointer('pointerdown', 1, center('START'))
    act(() => root.unmount())
    expect(last()).toBe(0)
    root = createRoot(container)
  })

  it('reports each change once', () => {
    render()
    pointer('pointerdown', 1, center('A'))
    pointer('pointermove', 1, { x: center('A').x + 1, y: center('A').y })
    expect(onChange).toHaveBeenCalledTimes(1)
  })

  it('ticks for button presses but not the D-pad', () => {
    render('light')
    const dpad = center('dpad')
    pointer('pointerdown', 1, { x: dpad.x + 30, y: dpad.y })
    pointer('pointermove', 1, { x: dpad.x + 30, y: dpad.y - 30 })
    expect(hapticTick).not.toHaveBeenCalled()
    pointer('pointerdown', 2, center('A'))
    expect(hapticTick).toHaveBeenCalledTimes(1)
    expect(hapticTick).toHaveBeenCalledWith('light')
    pointer('pointermove', 2, center('B'))
    expect(hapticTick).toHaveBeenCalledTimes(2)
    // Releases do not tick.
    pointer('pointerup', 2, center('B'))
    pointer('pointerup', 1, dpad)
    expect(hapticTick).toHaveBeenCalledTimes(2)
  })

  it('passes the chosen strength to the tick', () => {
    render('hard')
    pointer('pointerdown', 1, center('A'))
    expect(hapticTick).toHaveBeenCalledWith('hard')
  })

  it('does not tick when haptics are off', () => {
    render('off')
    pointer('pointerdown', 1, center('A'))
    expect(hapticTick).not.toHaveBeenCalled()
  })
})
