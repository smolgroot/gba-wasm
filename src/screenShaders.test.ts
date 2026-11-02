import { afterEach, describe, expect, it, vi } from 'vitest'
import { createScreenRenderer, isScreenShader, usesWebGl } from './screenShaders'

class FakeImageData {
  data: Uint8ClampedArray
  width: number
  height: number
  constructor(data: Uint8ClampedArray, width: number, height: number) {
    this.data = data
    this.width = width
    this.height = height
  }
}

/** A canvas whose 2D context records draws and which has no WebGL2, like an old browser. */
function createCanvas() {
  const putImageData = vi.fn()
  const canvas = document.createElement('canvas')
  const getContext = vi.fn((type: string) => (type === '2d' ? { putImageData } : null))
  canvas.getContext = getContext as unknown as HTMLCanvasElement['getContext']
  return { canvas, getContext, putImageData }
}

describe('screen shaders', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('accepts only known presets', () => {
    for (const shader of ['off', 'color', 'lcd', 'lcdBlur']) expect(isScreenShader(shader)).toBe(true)
    for (const value of ['crt', '', 'toString', null, 1, undefined]) expect(isScreenShader(value)).toBe(false)
  })

  it('draws with WebGL for every preset except off', () => {
    expect(usesWebGl('off')).toBe(false)
    expect(usesWebGl('color')).toBe(true)
    expect(usesWebGl('lcdBlur')).toBe(true)
  })

  it('draws raw frames on a 2D canvas when off, without asking for WebGL', () => {
    vi.stubGlobal('ImageData', FakeImageData)
    const { canvas, getContext, putImageData } = createCanvas()
    const renderer = createScreenRenderer(canvas, 'off')
    const pixels = new Uint8ClampedArray(240 * 160 * 4).fill(7)
    renderer.draw(pixels)

    expect(getContext).toHaveBeenCalledTimes(1)
    expect(getContext).toHaveBeenCalledWith('2d')
    expect(canvas.width).toBe(240)
    expect(canvas.height).toBe(160)
    const [image, x, y] = putImageData.mock.calls[0] as [FakeImageData, number, number]
    expect([image.width, image.height, x, y]).toEqual([240, 160, 0, 0])
    expect(image.data).toEqual(pixels)
  })

  it('falls back to the raw 2D canvas without WebGL2', () => {
    vi.stubGlobal('ImageData', FakeImageData)
    const { canvas, getContext, putImageData } = createCanvas()
    createScreenRenderer(canvas, 'lcd').draw(new Uint8ClampedArray(240 * 160 * 4))

    expect(getContext.mock.calls.map(([type]) => type)).toEqual(['webgl2', '2d'])
    expect(putImageData).toHaveBeenCalledTimes(1)
  })
})
