# Audio (APU)

`Apu` (`rust/src/apu.rs`) generates stereo 16-bit samples at **32,768 Hz**, one sample every 512 CPU cycles, as the clock advances. `MemoryMap` owns it like the timers and DMA. Sound registers stay in the I/O array, and the APU is notified of register writes, timer overflows, and elapsed cycles.

## Channels

- **Squares 1 and 2:** four duty patterns, an 11-bit frequency (step every `(2048 - f) * 16` cycles), a volume envelope, and a 64-step length counter. Square 1 adds frequency sweep, which writes the new frequency back to `SOUND1CNT_X` and stops the channel on overflow.
- **Wave:** 32 4-bit samples, or 64 when both banks are chained, stepped every `(2048 - f) * 8` cycles, at 0/25/50/75/100% volume, with a 256-step length counter. Register access to `0x04000090`-`0x0400009F` goes to the bank that is not playing.
- **Noise:** a 15- or 7-bit LFSR stepped at 524,288 Hz / r / 2^(s+1), with envelope and length.
- **DirectSound A and B:** 32-byte FIFOs written through `FIFO_A`/`FIFO_B`. Each FIFO plays a new sample when its selected timer (0 or 1) overflows. When 16 bytes or fewer remain, it requests DMA1 or DMA2 configured with special timing toward that FIFO; such a transfer always moves four words to the fixed FIFO address. The `SOUNDCNT_H` reset bits empty a FIFO.

A 512 Hz frame sequencer clocks length (256 Hz), sweep (128 Hz), and envelopes (64 Hz). A restart with the channel's DAC off (initial volume 0 and decreasing envelope, or wave playback disabled) leaves the channel silent. Clearing the master enable (`SOUNDCNT_X` bit 7) silences all output, clears the PSG registers `0x060`-`0x081`, and makes them ignore writes until it is set again. `SOUNDCNT_X` reads report the active channels in bits 0-3.

## Mixing

Per side, the enabled PSG channels (each ±volume) are summed, scaled by the master volume (1-8) and the `SOUNDCNT_H` PSG ratio (25/50/100%), so at full volume one channel spans about a quarter of the output range and one FIFO the full range, as on hardware. The enabled FIFO samples (signed 8-bit) are added at ×2 (50%) or ×4 (100%). The result is clamped to the 10-bit output range (±512) and widened to 16 bits. The DC bias level, `SOUNDBIAS` resolution, and analog filtering are not modeled.

## Approximations

- Channel waveforms are point-sampled at 32,768 Hz without band limiting, so very high square or noise frequencies alias.
- Timer overflows inside one CPU step are processed before that step's samples are generated, so a FIFO can change sample up to one instruction early.

## Browser output

`WasmEmulator::take_audio_samples` drains the buffered interleaved samples; the core keeps at most one second if nobody drains them. `src/audio.ts` plays them in an `AudioContext` created at 32,768 Hz, so the browser resamples to the device rate, through an AudioWorklet with a small queue. The worklet keeps a one-second ring buffer and reads it at a fractional position that advances by the game speed per output sample, with linear interpolation. At Fast (×2) audio plays twice as fast and an octave higher; at Slow (×0.5) it plays at half speed and an octave lower. Playback starts once about 60 ms of output are buffered. When more than about 190 ms are queued, the oldest audio is skipped. Both limits scale with the speed, so latency is the same at every speed. An underrun, such as while paused, outputs silence. Browsers start audio only after a user gesture, so the page resumes the context on the first key press or click. The **Sound** setting turns output off; samples are still drained.

## Tests

Unit tests cover square duty and frequency, length expiry and the status bits, FIFO playback on timer overflow with refill requests, master disable, and wave RAM bank access. A memory test covers EEPROM-style DMA routing; sound DMA reuses the same transfer path. In headless Chrome, audio was checked to arrive at 32,644 sample frames per second (real time is 32,768) with nothing pushed while paused.
