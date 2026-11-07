import { beforeEach, describe, expect, it } from 'vitest'
import { addPlayTime, formatPlayTime, getAllPlayTimes, getPlayTime } from './playStats'

beforeEach(() => localStorage.clear())

describe('play time storage', () => {
  it('starts at zero and accumulates per game', () => {
    expect(getPlayTime('Golden Sun')).toBe(0)
    expect(addPlayTime('Golden Sun', 1500)).toBe(1500)
    expect(addPlayTime('Golden Sun', 500)).toBe(2000)
    addPlayTime('Aladdin', 10)
    expect(getAllPlayTimes()).toEqual({ 'Golden Sun': 2000, Aladdin: 10 })
  })

  it('ignores empty or invalid increments', () => {
    addPlayTime('Aladdin', 0)
    addPlayTime('Aladdin', -5)
    addPlayTime('Aladdin', Number.NaN)
    expect(getPlayTime('Aladdin')).toBe(0)
  })

  it('recovers from corrupt stored data', () => {
    localStorage.setItem('pecten.playTime', '{nope')
    expect(getPlayTime('Aladdin')).toBe(0)
    localStorage.setItem('pecten.playTime', JSON.stringify({ A: 'x', B: -1, C: 9 }))
    expect(getAllPlayTimes()).toEqual({ C: 9 })
    expect(addPlayTime('A', 4)).toBe(4)
  })
})

describe('formatPlayTime', () => {
  it('reads naturally at every scale', () => {
    expect(formatPlayTime(0)).toBe('Not played yet')
    expect(formatPlayTime(30_000)).toBe('< 1 min')
    expect(formatPlayTime(42 * 60_000 + 59_000)).toBe('42 min')
    expect(formatPlayTime(185 * 60_000)).toBe('3 h 05 min')
  })
})
