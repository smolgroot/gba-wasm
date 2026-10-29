import { afterEach, describe, expect, it, vi } from 'vitest'
import { GBA_KEYS, attachKeyboard, dpadMaskForVector, gbaKeyForCode } from './keypad'

describe('GBA_KEYS', () => {
  it('matches the KEYINPUT bit order', () => {
    expect(Object.entries(GBA_KEYS).map(([name, bit]) => [name, Math.log2(bit)])).toEqual([
      ['A', 0], ['B', 1], ['SELECT', 2], ['START', 3], ['RIGHT', 4],
      ['LEFT', 5], ['UP', 6], ['DOWN', 7], ['R', 8], ['L', 9],
    ])
  })
})

describe('gbaKeyForCode', () => {
  it('maps physical key codes and ignores unmapped keys', () => {
    expect(gbaKeyForCode('KeyX')).toBe(GBA_KEYS.A)
    expect(gbaKeyForCode('KeyZ')).toBe(GBA_KEYS.B)
    expect(gbaKeyForCode('Enter')).toBe(GBA_KEYS.START)
    expect(gbaKeyForCode('ShiftRight')).toBe(GBA_KEYS.SELECT)
    expect(gbaKeyForCode('KeyA')).toBe(GBA_KEYS.L)
    expect(gbaKeyForCode('KeyS')).toBe(GBA_KEYS.R)
    expect(gbaKeyForCode('KeyQ')).toBeUndefined()
  })
})

describe('attachKeyboard', () => {
  let detach: (() => void) | undefined
  afterEach(() => {
    detach?.()
    detach = undefined
  })

  function attach() {
    const onChange = vi.fn<(pressed: number) => void>()
    detach = attachKeyboard(window, onChange)
    const last = () => onChange.mock.lastCall?.[0]
    return { onChange, last }
  }
  const key = (type: 'keydown' | 'keyup', code: string, init: KeyboardEventInit = {}) => {
    const event = new KeyboardEvent(type, { code, cancelable: true, ...init })
    window.dispatchEvent(event)
    return event
  }

  it('combines simultaneous keys and releases them independently', () => {
    const { last } = attach()
    key('keydown', 'KeyX')
    key('keydown', 'ArrowUp')
    expect(last()).toBe(GBA_KEYS.A | GBA_KEYS.UP)
    key('keyup', 'KeyX')
    expect(last()).toBe(GBA_KEYS.UP)
    key('keyup', 'ArrowUp')
    expect(last()).toBe(0)
  })

  it('keeps Select held while the other Shift key is down', () => {
    const { last } = attach()
    key('keydown', 'ShiftLeft')
    key('keydown', 'ShiftRight')
    key('keyup', 'ShiftLeft')
    expect(last()).toBe(GBA_KEYS.SELECT)
    key('keyup', 'ShiftRight')
    expect(last()).toBe(0)
  })

  it('reports only transitions, not auto-repeat', () => {
    const { onChange } = attach()
    key('keydown', 'KeyX')
    key('keydown', 'KeyX', { repeat: true })
    expect(onChange).toHaveBeenCalledTimes(1)
    key('keyup', 'KeyQ')
    expect(onChange).toHaveBeenCalledTimes(1)
  })

  it('prevents the default action only for mapped keys', () => {
    attach()
    expect(key('keydown', 'ArrowDown').defaultPrevented).toBe(true)
    expect(key('keydown', 'KeyQ').defaultPrevented).toBe(false)
  })

  it('ignores browser shortcuts and typing in text fields', () => {
    const { onChange } = attach()
    key('keydown', 'KeyA', { ctrlKey: true })
    key('keydown', 'KeyS', { metaKey: true })
    const input = document.body.appendChild(document.createElement('input'))
    input.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyX', bubbles: true }))
    input.remove()
    expect(onChange).not.toHaveBeenCalled()
  })

  it('releases every key when the window loses focus', () => {
    const { last } = attach()
    key('keydown', 'KeyX')
    key('keydown', 'ArrowLeft')
    window.dispatchEvent(new Event('blur'))
    expect(last()).toBe(0)
    // The lost key-ups arriving later must not report anything stale.
    key('keyup', 'KeyX')
    expect(last()).toBe(0)
  })

  it('releases every key when the page is hidden', () => {
    const { last } = attach()
    key('keydown', 'Enter')
    document.dispatchEvent(new Event('visibilitychange'))
    expect(last()).toBe(0)
  })

  it('stops reporting once detached', () => {
    const { onChange } = attach()
    detach?.()
    detach = undefined
    key('keydown', 'KeyX')
    window.dispatchEvent(new Event('blur'))
    expect(onChange).not.toHaveBeenCalled()
  })
})

describe('dpadMaskForVector', () => {
  const { UP, DOWN, LEFT, RIGHT } = GBA_KEYS
  const at = (degrees: number, distance = 1) => {
    const radians = (degrees * Math.PI) / 180
    // y points down on screen, so 90° is up.
    return dpadMaskForVector(Math.cos(radians) * distance, -Math.sin(radians) * distance)
  }

  it('presses nothing inside the dead zone', () => {
    expect(dpadMaskForVector(0, 0)).toBe(0)
    expect(dpadMaskForVector(0.1, -0.1)).toBe(0)
    expect(at(0, 0.25)).toBe(RIGHT)
  })

  it('splits the circle into eight 45° sectors', () => {
    expect(at(0)).toBe(RIGHT)
    expect(at(45)).toBe(UP | RIGHT)
    expect(at(90)).toBe(UP)
    expect(at(135)).toBe(UP | LEFT)
    expect(at(180)).toBe(LEFT)
    expect(at(225)).toBe(DOWN | LEFT)
    expect(at(270)).toBe(DOWN)
    expect(at(315)).toBe(DOWN | RIGHT)
    // Sector edges sit at 22.5° either side of each direction.
    expect(at(22)).toBe(RIGHT)
    expect(at(23)).toBe(UP | RIGHT)
    expect(at(67)).toBe(UP | RIGHT)
    expect(at(68)).toBe(UP)
  })

  it('never presses opposite directions together', () => {
    for (let degrees = 0; degrees < 360; degrees += 1) {
      const mask = at(degrees, 3)
      expect(mask & (UP | DOWN)).not.toBe(UP | DOWN)
      expect(mask & (LEFT | RIGHT)).not.toBe(LEFT | RIGHT)
      expect(mask).not.toBe(0)
    }
  })
})
