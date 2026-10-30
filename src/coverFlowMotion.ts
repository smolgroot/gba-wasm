// Physics for the cover flow scroll position, measured in covers (0 = first cover centred).

// Stiffness of the spring that settles the position on a cover; damping keeps it critically damped (no wobble).
const SPRING_STIFFNESS = 140
const SPRING_DAMPING = 2 * Math.sqrt(SPRING_STIFFNESS)
// How far, in seconds of travel at release speed, a flick carries the position before it settles.
const INERTIA_SECONDS = 0.28
// Fraction of a drag past either end that moves the covers, so the ends feel elastic.
const OVERSCROLL_RESISTANCE = 0.3
// The spring stops once this close to its target and this slow.
const REST_DISTANCE = 0.002
const REST_SPEED = 0.01

export type SpringState = { position: number; velocity: number }

/** Advances a critically damped spring towards `target` by `seconds` (semi-implicit Euler). */
export function stepSpring({ position, velocity }: SpringState, target: number, seconds: number): SpringState {
  const acceleration = -SPRING_STIFFNESS * (position - target) - SPRING_DAMPING * velocity
  const nextVelocity = velocity + acceleration * seconds
  return { position: position + nextVelocity * seconds, velocity: nextVelocity }
}

export function isAtRest({ position, velocity }: SpringState, target: number) {
  return Math.abs(position - target) < REST_DISTANCE && Math.abs(velocity) < REST_SPEED
}

/** The cover a release at `velocity` (covers per second) comes to rest on. */
export function inertiaTarget(position: number, velocity: number, last: number) {
  return clamp(Math.round(position + velocity * INERTIA_SECONDS), 0, last)
}

/** Lets the position run past the first and last cover with resistance. */
export function withOverscroll(position: number, last: number) {
  if (position < 0) return position * OVERSCROLL_RESISTANCE
  if (position > last) return last + (position - last) * OVERSCROLL_RESISTANCE
  return position
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}

/** Tracks recent positions to measure the release velocity of a drag or scroll, in covers per second. */
export class VelocityTracker {
  // Only motion this recent counts, so a drag that stops before release does not fling.
  private static readonly WINDOW_MS = 100
  private samples: { time: number; position: number }[] = []

  add(time: number, position: number) {
    this.samples.push({ time, position })
    while (this.samples.length > 2 && time - this.samples[0].time > VelocityTracker.WINDOW_MS) this.samples.shift()
  }

  velocity(now: number) {
    const recent = this.samples.filter((sample) => now - sample.time <= VelocityTracker.WINDOW_MS)
    if (recent.length < 2) return 0
    const first = recent[0]
    const last = recent[recent.length - 1]
    const elapsed = (last.time - first.time) / 1000
    return elapsed > 0 ? (last.position - first.position) / elapsed : 0
  }

  reset() {
    this.samples = []
  }
}
