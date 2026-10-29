// Plays the core's interleaved 16-bit stereo samples. The AudioContext runs at the core's
// sample rate (32,768 Hz) so the browser resamples to the output device.

// Buffering limits, in milliseconds of output. In input frames they scale with the playback
// rate, so the latency stays the same at every game speed.
const TARGET_BUFFERED = 3072 // ~94 ms at 1x queued before playback starts, and held there
const MAX_BUFFERED = 8192 // ~250 ms at 1x; older audio is dropped beyond this
const RING_CAPACITY = 32768 // one second of input at 1x, enough for 2x with headroom
// The emulator is paced by the display clock and playback by the audio device's clock. They
// drift apart, so playback is sped up or slowed down by at most this fraction (under 9 cents
// of pitch) to keep the queue near its target instead of running dry or overflowing.
const MAX_RATE_CORRECTION = 0.005
// Per render quantum (128 frames, ~4 ms): smooths the queue level over ~0.4 s so the bursts
// of one emulated frame at a time do not modulate the pitch.
const LEVEL_SMOOTHING = 0.01
// Per output frame: fades towards silence on underrun (~3 ms), since a jump to 0 clicks.
const UNDERRUN_DECAY = 0.99

const WORKLET_SOURCE = `
class GbaAudioProcessor extends AudioWorkletProcessor {
  constructor() {
    super()
    this.left = new Float32Array(${RING_CAPACITY})
    this.right = new Float32Array(${RING_CAPACITY})
    this.written = 0 // input frames written in total
    this.position = 0 // fractional input frame being played
    this.rate = 1 // input frames consumed per output frame (the game speed)
    this.playing = false
    this.level = 0 // smoothed queue length in output frames
    this.lastLeft = 0
    this.lastRight = 0
    this.port.onmessage = ({ data }) => {
      if (!(data instanceof Float32Array)) {
        this.rate = data.rate
        return
      }
      for (let index = 0; index < data.length; index += 2) {
        const slot = this.written % ${RING_CAPACITY}
        this.left[slot] = data[index]
        this.right[slot] = data[index + 1]
        this.written += 1
      }
      // Emulation running far ahead of the device: skip the oldest audio to bound latency.
      if (this.written - this.position > ${MAX_BUFFERED} * this.rate) {
        this.position = this.written - ${TARGET_BUFFERED} * this.rate
        this.level = ${TARGET_BUFFERED}
      }
    }
  }

  process(_inputs, outputs) {
    const [left, right] = outputs[0]
    const queued = (this.written - this.position) / this.rate
    if (!this.playing && queued >= ${TARGET_BUFFERED}) {
      this.playing = true
      this.level = queued
    }
    this.level += (queued - this.level) * ${LEVEL_SMOOTHING}
    const error = this.level / ${TARGET_BUFFERED} - 1
    const correction = Math.min(Math.max(error * ${MAX_RATE_CORRECTION}, -${MAX_RATE_CORRECTION}), ${MAX_RATE_CORRECTION})
    const step = this.rate * (1 + correction)
    for (let index = 0; index < left.length; index += 1) {
      // Interpolation needs the frame after the current one.
      if (!this.playing || this.written - this.position < 2) {
        // Underrun (paused or slow emulation): fade out and wait to refill.
        this.lastLeft *= ${UNDERRUN_DECAY}
        this.lastRight *= ${UNDERRUN_DECAY}
        left[index] = this.lastLeft
        right[index] = this.lastRight
        this.playing = false
        continue
      }
      const frame = Math.floor(this.position)
      const fraction = this.position - frame
      const current = frame % ${RING_CAPACITY}
      const next = (frame + 1) % ${RING_CAPACITY}
      left[index] = this.left[current] + (this.left[next] - this.left[current]) * fraction
      right[index] = this.right[current] + (this.right[next] - this.right[current]) * fraction
      this.lastLeft = left[index]
      this.lastRight = right[index]
      // Consuming input faster or slower than real time changes tempo and pitch together,
      // like a tape played at another speed.
      this.position += step
    }
    return true
  }
}
registerProcessor('gba-audio', GbaAudioProcessor)
`

export class AudioOutput {
  private readonly context: AudioContext
  private readonly node: AudioWorkletNode
  private readonly gain: GainNode

  private constructor(context: AudioContext, node: AudioWorkletNode, gain: GainNode) {
    this.context = context
    this.node = node
    this.gain = gain
  }

  /** Throws when the browser lacks AudioWorklet support (for example, an insecure origin). */
  static async create(sampleRate: number, volume: number) {
    const context = new AudioContext({ sampleRate, latencyHint: 'interactive' })
    const moduleUrl = URL.createObjectURL(new Blob([WORKLET_SOURCE], { type: 'text/javascript' }))
    try {
      await context.audioWorklet.addModule(moduleUrl)
    } catch (error) {
      void context.close()
      throw error
    } finally {
      URL.revokeObjectURL(moduleUrl)
    }
    const node = new AudioWorkletNode(context, 'gba-audio', { outputChannelCount: [2] })
    const gain = new GainNode(context, { gain: volume })
    node.connect(gain).connect(context.destination)
    return new AudioOutput(context, node, gain)
  }

  /** Plays audio `rate` times faster (higher pitch) or slower (lower pitch) than real time. */
  setPlaybackRate(rate: number) {
    this.node.port.postMessage({ rate })
  }

  /** Linear output gain from 0 (silent) to 1 (the core's full level). */
  setVolume(volume: number) {
    // A short ramp avoids clicks while the slider is dragged.
    this.gain.gain.setTargetAtTime(volume, this.context.currentTime, 0.02)
  }

  push(samples: Int16Array) {
    if (samples.length === 0) return
    const floats = new Float32Array(samples.length)
    for (let index = 0; index < samples.length; index += 1) floats[index] = samples[index] / 32768
    this.node.port.postMessage(floats, [floats.buffer])
  }

  /** Browsers start audio suspended until a user gesture; call this from input handlers. */
  resume() {
    if (this.context.state === 'suspended') void this.context.resume()
  }

  close() {
    this.node.disconnect()
    this.gain.disconnect()
    void this.context.close()
  }
}
