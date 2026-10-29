import { afterEach, describe, expect, it, vi } from 'vitest'
import { AudioOutput, WORKLET_SOURCE } from './audio'

const QUANTUM = 128
const TARGET_BUFFERED = 3072
const MAX_BUFFERED = 8192

type Port = { onmessage: ((event: { data: unknown }) => void) | null }
type Processor = { port: Port; process: (inputs: unknown[], outputs: Float32Array[][]) => boolean }

/** Runs the worklet source with stand-ins for the AudioWorkletGlobalScope it expects. */
function createProcessor() {
  class AudioWorkletProcessor {
    port: Port = { onmessage: null }
  }
  let Constructor: (new () => Processor) | undefined
  new Function('AudioWorkletProcessor', 'registerProcessor', WORKLET_SOURCE)(
    AudioWorkletProcessor,
    (name: string, processor: new () => Processor) => {
      expect(name).toBe('gba-audio')
      Constructor = processor
    },
  )
  const processor = new Constructor!()
  const send = (data: unknown) => processor.port.onmessage!({ data })
  return {
    /** Queues `frames` stereo frames; `sample(i)` gives the left value, right is its negation. */
    push(frames: number, sample: (index: number) => number, offset = 0) {
      const data = new Float32Array(frames * 2)
      for (let index = 0; index < frames; index += 1) {
        data[index * 2] = sample(offset + index)
        data[index * 2 + 1] = -sample(offset + index)
      }
      send(data)
    },
    setRate: (rate: number) => send({ rate }),
    render() {
      const left = new Float32Array(QUANTUM)
      const right = new Float32Array(QUANTUM)
      expect(processor.process([], [[left, right]])).toBe(true)
      return { left, right }
    },
  }
}

const ramp = (index: number) => index / 65536

describe('audio worklet processor', () => {
  it('stays silent until enough audio is queued, then plays it in order', () => {
    const processor = createProcessor()
    processor.push(TARGET_BUFFERED - 1, ramp)
    expect(processor.render().left.every((sample) => sample === 0)).toBe(true)

    processor.push(1, ramp, TARGET_BUFFERED - 1)
    const { left, right } = processor.render()
    for (let index = 0; index < QUANTUM; index += 1) {
      expect(left[index]).toBeCloseTo(ramp(index), 6)
      expect(right[index]).toBeCloseTo(-ramp(index), 6)
    }
  })

  it('fades out on underrun instead of clicking, then waits for a full refill', () => {
    const processor = createProcessor()
    processor.push(TARGET_BUFFERED, () => 0.5)
    let previous = 0.5
    let largestJump = 0
    // Play the queue dry, then keep rendering as if emulation were paused.
    for (let quantum = 0; quantum < 60; quantum += 1) {
      for (const sample of processor.render().left) {
        largestJump = Math.max(largestJump, Math.abs(sample - previous))
        previous = sample
      }
    }
    expect(largestJump).toBeLessThan(0.01)
    expect(Math.abs(previous)).toBeLessThan(1e-3)

    // A trickle of audio after an underrun is not enough to restart playback.
    processor.push(QUANTUM * 4, () => 0.5)
    expect(Math.max(...processor.render().left.map(Math.abs))).toBeLessThan(1e-3)
  })

  it('drops the oldest audio when emulation runs far ahead of the device', () => {
    const processor = createProcessor()
    const pushed = MAX_BUFFERED + 2000
    processor.push(pushed, ramp)
    // Playback resumes TARGET_BUFFERED frames behind the newest audio.
    expect(processor.render().left[0]).toBeCloseTo(ramp(pushed - TARGET_BUFFERED), 6)
  })

  it('consumes input at the playback rate', () => {
    const processor = createProcessor()
    processor.setRate(2)
    processor.push(TARGET_BUFFERED * 2, ramp)
    const { left } = processor.render()
    for (let index = 0; index < QUANTUM; index += 1) expect(left[index]).toBeCloseTo(ramp(index * 2), 6)
  })

  it('keeps playing smoothly across ring-buffer wraps', () => {
    const processor = createProcessor()
    const tone = (index: number) => 0.5 * Math.sin(index / 50)
    // Production matches consumption (4 quanta per pushed chunk), so the queue never runs dry.
    let written = 0
    let previous: number | null = null
    let largestJump = 0
    let loudest = 0
    for (let frame = 0; frame < 200; frame += 1) {
      processor.push(QUANTUM * 4, tone, written)
      written += QUANTUM * 4
      for (let quantum = 0; quantum < 4; quantum += 1) {
        for (const sample of processor.render().left) {
          expect(Number.isFinite(sample)).toBe(true)
          if (previous !== null) largestJump = Math.max(largestJump, Math.abs(sample - previous))
          if (frame >= 190) loudest = Math.max(loudest, Math.abs(sample))
          previous = sample
        }
      }
    }
    expect(loudest).toBeGreaterThan(0.49)
    expect(written).toBeGreaterThan(3 * 32768)
    // The tone changes by at most 0.01 per frame; a skip or stale slot would jump further.
    expect(largestJump).toBeLessThan(0.0125)
  })
})

/** Records the Web Audio calls `AudioOutput` makes. */
function stubWebAudio({ failModule = false, state = 'suspended' } = {}) {
  const calls: string[] = []
  const posted: { data: unknown; transfer?: Transferable[] }[] = []
  class FakeAudioContext {
    state = state
    currentTime = 1.5
    destination = {}
    audioWorklet = {
      addModule: vi.fn(async () => {
        calls.push('addModule')
        if (failModule) throw new Error('AudioWorklet unavailable')
      }),
    }
    constructor(options: AudioContextOptions) {
      calls.push(`context ${options.sampleRate}`)
    }
    resume = vi.fn(async () => { calls.push('resume') })
    close = vi.fn(async () => { calls.push('close') })
  }
  class FakeNode {
    connect = vi.fn((target: unknown) => target)
    disconnect = vi.fn(() => calls.push(`disconnect ${this.constructor.name}`))
  }
  class FakeWorkletNode extends FakeNode {
    port = { postMessage: (data: unknown, transfer?: Transferable[]) => posted.push({ data, transfer }) }
  }
  class FakeGainNode extends FakeNode {
    gain: { value: number; setTargetAtTime: ReturnType<typeof vi.fn> }
    constructor(_context: unknown, options: { gain: number }) {
      super()
      this.gain = { value: options.gain, setTargetAtTime: vi.fn() }
    }
  }
  vi.stubGlobal('AudioContext', FakeAudioContext)
  vi.stubGlobal('AudioWorkletNode', FakeWorkletNode)
  vi.stubGlobal('GainNode', FakeGainNode)
  URL.createObjectURL = vi.fn(() => 'blob:worklet')
  URL.revokeObjectURL = vi.fn()
  return { calls, posted }
}

describe('AudioOutput', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('runs the context at the core sample rate and starts suspended until resumed', async () => {
    const { calls } = stubWebAudio()
    const output = await AudioOutput.create(32768, 0.5)
    expect(calls).toEqual(['context 32768', 'addModule'])
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:worklet')
    output.resume()
    expect(calls).toContain('resume')
  })

  it('does not resume a running context', async () => {
    const { calls } = stubWebAudio({ state: 'running' })
    const output = await AudioOutput.create(32768, 1)
    output.resume()
    expect(calls).not.toContain('resume')
  })

  it('closes the context and rethrows when the worklet cannot load', async () => {
    const { calls } = stubWebAudio({ failModule: true })
    await expect(AudioOutput.create(32768, 1)).rejects.toThrow('AudioWorklet unavailable')
    expect(calls).toContain('close')
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:worklet')
  })

  it('converts samples to floats and transfers them to the worklet', async () => {
    const { posted } = stubWebAudio()
    const output = await AudioOutput.create(32768, 1)
    output.push(new Int16Array([]))
    expect(posted).toHaveLength(0)
    output.push(new Int16Array([-32768, 0, 16384, 32767]))
    const [{ data, transfer }] = posted
    expect(Array.from(data as Float32Array)).toEqual([-1, 0, 0.5, 32767 / 32768])
    expect(transfer).toEqual([(data as Float32Array).buffer])
  })

  it('forwards playback rate and ramps volume changes', async () => {
    const { posted } = stubWebAudio()
    const output = await AudioOutput.create(32768, 1)
    output.setPlaybackRate(2)
    expect(posted.at(-1)?.data).toEqual({ rate: 2 })
    output.setVolume(0.25)
    const gain = (output as unknown as { gain: { gain: { setTargetAtTime: ReturnType<typeof vi.fn> } } }).gain
    expect(gain.gain.setTargetAtTime).toHaveBeenCalledWith(0.25, 1.5, 0.02)
  })

  it('disconnects its nodes and closes the context on teardown', async () => {
    const { calls } = stubWebAudio()
    const output = await AudioOutput.create(32768, 1)
    output.close()
    expect(calls.slice(-3)).toEqual(['disconnect FakeWorkletNode', 'disconnect FakeGainNode', 'close'])
  })
})
