// Optional presentation shaders for the game screen, applied after the core has rendered a frame.
//
// `off` draws the raw frame on a 2D canvas, as the emulator always has. The other presets draw it
// with WebGL2 at the display resolution, so effects can be finer than one GBA pixel:
// - colour correction: the GBA's unlit LCD showed washed-out, less saturated colours, and games
//   were coloured for it; the community "gba-color" matrix approximates that screen;
// - pixel grid: the dark gaps between the LCD's cells;
// - frame blending: a mix of the last two frames, like the slow LCD response. Games such as
//   Golden Sun flicker sprites on alternate frames for transparency, which this smooths out.
// A canvas keeps the type of its first context, so callers need a new canvas when switching
// between `off` and a shader (see `usesWebGl`).

export type ScreenShader = 'off' | 'color' | 'lcd' | 'lcdBlur'

const SHADER_EFFECTS: Record<Exclude<ScreenShader, 'off'>, { color: boolean; grid: boolean; blend: boolean }> = {
  color: { color: true, grid: false, blend: false },
  lcd: { color: true, grid: true, blend: false },
  lcdBlur: { color: true, grid: true, blend: true },
}

const WIDTH = 240
const HEIGHT = 160

export function isScreenShader(value: unknown): value is ScreenShader {
  return value === 'off' || (typeof value === 'string' && Object.hasOwn(SHADER_EFFECTS, value))
}

/** Whether a preset draws with WebGL, so needs a different canvas from `off`. */
export function usesWebGl(shader: ScreenShader) {
  return shader !== 'off'
}

export type ScreenRenderer = {
  /** Draws a 240 x 160 RGBA frame. */
  draw: (pixels: Uint8ClampedArray) => void
  dispose: () => void
}

function create2dRenderer(canvas: HTMLCanvasElement): ScreenRenderer {
  canvas.width = WIDTH
  canvas.height = HEIGHT
  const context = canvas.getContext('2d')
  return {
    draw: (pixels) => context?.putImageData(new ImageData(new Uint8ClampedArray(pixels), WIDTH, HEIGHT), 0, 0),
    dispose: () => {},
  }
}

const VERTEX_SHADER = `#version 300 es
in vec2 aPosition;
out vec2 vUv;
void main() {
  // Texture row 0 is the top of the frame.
  vUv = vec2(aPosition.x * 0.5 + 0.5, 0.5 - aPosition.y * 0.5);
  gl_Position = vec4(aPosition, 0.0, 1.0);
}`

const FRAGMENT_SHADER = `#version 300 es
precision mediump float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uCurrent;
uniform sampler2D uPrevious;
uniform bool uColor;
uniform bool uGrid;
uniform bool uBlend;
// Device pixels per GBA pixel.
uniform float uScale;
const vec2 SCREEN = vec2(${WIDTH}.0, ${HEIGHT}.0);
// Columns are the contributions of the input red, green and blue.
const mat3 GBA_LCD = mat3(
  0.80, 0.135, 0.195,
  0.275, 0.64, 0.155,
  -0.075, 0.225, 0.65);
void main() {
  vec3 color = texture(uCurrent, vUv).rgb;
  if (uBlend) color = mix(color, texture(uPrevious, vUv).rgb, 0.5);
  if (uColor) color = pow(clamp(GBA_LCD * pow(color, vec3(2.2)), 0.0, 1.0), vec3(1.0 / 2.2));
  if (uGrid) {
    // Distance in device pixels to the nearest cell edge; the grid fades out on small screens,
    // where lines thinner than a device pixel would only add moire.
    vec2 cell = fract(vUv * SCREEN);
    vec2 edge = min(cell, 1.0 - cell) * uScale;
    float line = 1.0 - smoothstep(0.0, 1.0, min(edge.x, edge.y));
    color *= 1.0 - 0.35 * line * smoothstep(2.0, 4.0, uScale);
  }
  outColor = vec4(color, 1.0);
}`

function compile(gl: WebGL2RenderingContext, type: number, source: string) {
  const shader = gl.createShader(type)
  if (!shader) throw new Error('Could not create shader')
  gl.shaderSource(shader, source)
  gl.compileShader(shader)
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) ?? 'Shader compile failed')
  return shader
}

function createFrameTexture(gl: WebGL2RenderingContext) {
  const texture = gl.createTexture()
  gl.bindTexture(gl.TEXTURE_2D, texture)
  gl.texStorage2D(gl.TEXTURE_2D, 1, gl.RGBA8, WIDTH, HEIGHT)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
  return texture
}

function createWebGlRenderer(canvas: HTMLCanvasElement, gl: WebGL2RenderingContext, effects: { color: boolean; grid: boolean; blend: boolean }): ScreenRenderer {
  const program = gl.createProgram()
  gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, VERTEX_SHADER))
  gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER))
  gl.linkProgram(program)
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) ?? 'Shader link failed')
  gl.useProgram(program)

  const quad = gl.createBuffer()
  gl.bindBuffer(gl.ARRAY_BUFFER, quad)
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW)
  const position = gl.getAttribLocation(program, 'aPosition')
  gl.enableVertexAttribArray(position)
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0)

  gl.uniform1i(gl.getUniformLocation(program, 'uCurrent'), 0)
  gl.uniform1i(gl.getUniformLocation(program, 'uPrevious'), 1)
  gl.uniform1i(gl.getUniformLocation(program, 'uColor'), Number(effects.color))
  gl.uniform1i(gl.getUniformLocation(program, 'uGrid'), Number(effects.grid))
  gl.uniform1i(gl.getUniformLocation(program, 'uBlend'), Number(effects.blend))
  const scaleLocation = gl.getUniformLocation(program, 'uScale')

  let current = createFrameTexture(gl)
  let previous = createFrameTexture(gl)
  let hasFrame = false

  const render = () => {
    if (!hasFrame) return
    gl.viewport(0, 0, canvas.width, canvas.height)
    gl.uniform1f(scaleLocation, canvas.width / WIDTH)
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, current)
    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, previous)
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4)
  }

  const upload = (texture: WebGLTexture, pixels: Uint8ClampedArray) => {
    gl.bindTexture(gl.TEXTURE_2D, texture)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, WIDTH, HEIGHT, gl.RGBA, gl.UNSIGNED_BYTE, pixels)
  }

  // Render at the displayed size in device pixels, so the grid lines stay sharp.
  const resize = () => {
    const ratio = window.devicePixelRatio || 1
    const width = Math.max(WIDTH, Math.round(canvas.clientWidth * ratio))
    const height = Math.max(HEIGHT, Math.round(canvas.clientHeight * ratio))
    if (canvas.width === width && canvas.height === height) return
    canvas.width = width
    canvas.height = height
    render()
  }
  const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(resize) : null
  observer?.observe(canvas)
  resize()

  return {
    draw: (pixels) => {
      if (hasFrame) {
        ;[current, previous] = [previous, current]
      } else {
        // Blend the first frame with itself rather than with an empty texture.
        upload(previous, pixels)
        hasFrame = true
      }
      upload(current, pixels)
      render()
    },
    dispose: () => {
      observer?.disconnect()
      gl.deleteTexture(current)
      gl.deleteTexture(previous)
      gl.deleteBuffer(quad)
      gl.deleteProgram(program)
    },
  }
}

/** Creates the renderer for a preset; falls back to the raw 2D canvas without WebGL2. */
export function createScreenRenderer(canvas: HTMLCanvasElement, shader: ScreenShader): ScreenRenderer {
  if (shader !== 'off') {
    const gl = canvas.getContext('webgl2', { alpha: false, antialias: false })
    if (gl) {
      try {
        return createWebGlRenderer(canvas, gl, SHADER_EFFECTS[shader])
      } catch (error) {
        // The canvas now holds a WebGL context, so it can only show a blank screen.
        console.warn('Could not start the screen shader', error)
        return { draw: () => {}, dispose: () => {} }
      }
    }
  }
  return create2dRenderer(canvas)
}
