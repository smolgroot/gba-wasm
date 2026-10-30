import { describe, expect, it } from 'vitest'
import { VelocityTracker, inertiaTarget, isAtRest, stepSpring, withOverscroll, type SpringState } from './coverFlowMotion'

function settle(state: SpringState, target: number) {
  let frames = 0
  let maxPosition = state.position
  while (!isAtRest(state, target) && frames < 600) {
    state = stepSpring(state, target, 1 / 60)
    maxPosition = Math.max(maxPosition, state.position)
    frames++
  }
  return { state, frames, maxPosition }
}

describe('stepSpring', () => {
  it('settles on the target within a second without overshooting', () => {
    const { state, frames, maxPosition } = settle({ position: 0, velocity: 0 }, 1)
    expect(state.position).toBeCloseTo(1, 2)
    expect(frames).toBeLessThan(60)
    expect(maxPosition).toBeLessThanOrEqual(1.001)
  })
})

describe('inertiaTarget', () => {
  it('carries a flick past the nearest cover and stays within the list', () => {
    expect(inertiaTarget(2.2, 0, 8)).toBe(2)
    expect(inertiaTarget(2.2, 12, 8)).toBe(6)
    expect(inertiaTarget(2.2, 60, 8)).toBe(8)
    expect(inertiaTarget(2.2, -60, 8)).toBe(0)
  })
})

describe('withOverscroll', () => {
  it('resists dragging past either end', () => {
    expect(withOverscroll(4, 8)).toBe(4)
    expect(withOverscroll(-1, 8)).toBeCloseTo(-0.3)
    expect(withOverscroll(9, 8)).toBeCloseTo(8.3)
  })
})

describe('VelocityTracker', () => {
  it('measures recent motion only', () => {
    const tracker = new VelocityTracker()
    tracker.add(0, 0)
    tracker.add(50, 0.5)
    tracker.add(100, 1)
    expect(tracker.velocity(100)).toBeCloseTo(10)
    expect(tracker.velocity(400)).toBe(0)
  })
})
