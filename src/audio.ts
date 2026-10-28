// Plays the core's interleaved 16-bit stereo samples. The AudioContext runs at the core's
// sample rate (32,768 Hz) so the browser resamples to the output device.

// Buffering limits, in stereo frames at the core's sample rate.
const START_THRESHOLD = 2048 // ~60 ms queued before playback starts or restarts
const MAX_BUFFERED = 6144 // ~190 ms; older audio is dropped beyond this

const WORKLET_SOURCE = `
class GbaAudioProcessor extends AudioWorkletProcessor {
  constructor() {
    super()
    this.chunks = []
    this.offset = 0
    this.buffered = 0
    this.playing = false
    this.port.onmessage = (event) => {
      this.chunks.push(event.data)
      this.buffered += event.data.length / 2
      // Emulation running ahead of the device: drop the oldest audio to bound latency.
      while (this.buffered > ${MAX_BUFFERED} && this.chunks.length > 1) {
        this.buffered -= (this.chunks.shift().length - this.offset) / 2
        this.offset = 0
      }
    }
  }

  process(_inputs, outputs) {
    const [left, right] = outputs[0]
    if (!this.playing && this.buffered >= ${START_THRESHOLD}) this.playing = true
    for (let index = 0; index < left.length; index += 1) {
      const chunk = this.chunks[0]
      if (!this.playing || !chunk) {
        // Underrun (paused or slow emulation): output silence and wait to refill.
        left[index] = 0
        right[index] = 0
        this.playing = false
        continue
      }
      left[index] = chunk[this.offset]
      right[index] = chunk[this.offset + 1]
      this.offset += 2
      this.buffered -= 1
      if (this.offset >= chunk.length) {
        this.chunks.shift()
        this.offset = 0
      }
    }
    return true
  }
}
registerProcessor('gba-audio', GbaAudioProcessor)
`

export class AudioOutput {
  private readonly context: AudioContext
  private readonly node: AudioWorkletNode

  private constructor(context: AudioContext, node: AudioWorkletNode) {
    this.context = context
    this.node = node
  }

  /** Throws when the browser lacks AudioWorklet support (for example, an insecure origin). */
  static async create(sampleRate: number) {
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
    node.connect(context.destination)
    return new AudioOutput(context, node)
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
    void this.context.close()
  }
}
