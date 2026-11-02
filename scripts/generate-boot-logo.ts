// Converts public/logo.png into the 64x64, 16-color sprite drawn by the HLE BIOS boot
// animation and writes it to rust/src/boot_logo.rs. Requires ImageMagick 7 (`magick`) on PATH.
// Usage: pnpm run bootlogo:generate
import { spawnSync } from 'node:child_process'
import { writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const SIZE = 64
// Index 0 is transparent in a 4bpp object palette.
const MAX_COLORS = 15
const source = fileURLToPath(new URL('../public/logo.png', import.meta.url))
const output = fileURLToPath(new URL('../rust/src/boot_logo.rs', import.meta.url))

function magick(args: string[]): Buffer {
  const result = spawnSync('magick', args, { maxBuffer: 1 << 20 })
  if (result.status !== 0) throw new Error(`magick exited with ${result.status ?? result.error}: ${result.stderr}`)
  return result.stdout
}

// Crop to the opaque artwork, then reduce the upscaled pixel art to its original scale.
const crop = [source, '-channel', 'A', '-threshold', '50%', '+channel', '-trim', '+repage', '-filter', 'Box', '-resize', `${SIZE}x${SIZE}`]
const [width, height] = magick([...crop, '-format', '%w %h', 'info:']).toString().split(' ').map(Number)
const rgb = magick([...crop, '-alpha', 'off', '+dither', '-colors', String(MAX_COLORS), '-depth', '8', 'rgb:-'])
const alpha = magick([...crop, '-alpha', 'extract', '-threshold', '50%', '-depth', '8', 'gray:-'])

// RGB555, as stored in GBA palette RAM.
const toRgb555 = (offset: number) => (rgb[offset] >> 3) | ((rgb[offset + 1] >> 3) << 5) | ((rgb[offset + 2] >> 3) << 10)
const palette: number[] = []
const rows: string[] = []
const top = Math.floor((SIZE - height) / 2)
const left = Math.floor((SIZE - width) / 2)
for (let y = 0; y < SIZE; y += 1) {
  let row = ''
  for (let x = 0; x < SIZE; x += 1) {
    const [sx, sy] = [x - left, y - top]
    const inside = sx >= 0 && sx < width && sy >= 0 && sy < height
    if (!inside || alpha[sy * width + sx] < 128) {
      row += '0'
      continue
    }
    const color = toRgb555((sy * width + sx) * 3)
    let index = palette.indexOf(color)
    if (index < 0) index = palette.push(color) - 1
    if (index >= MAX_COLORS) throw new Error(`logo has more than ${MAX_COLORS} colors after RGB555 conversion`)
    row += (index + 1).toString(16).toUpperCase()
  }
  rows.push(row)
}

const hex = (value: number) => `0x${value.toString(16).toUpperCase().padStart(4, '0')}`
writeFileSync(output, `//! Pecten logo for the boot animation, generated from \`public/logo.png\` by
//! \`scripts/generate-boot-logo.ts\`. Do not edit by hand.

/// Object palette 0 in RGB555. Entry 0 is transparent.
pub const LOGO_PALETTE: [u16; 16] = [
    0x0000, ${palette.map(hex).join(', ')}${', 0x0000'.repeat(MAX_COLORS - palette.length)},
];

/// ${SIZE}x${SIZE} pixels, one palette index (hex digit) per pixel.
pub const LOGO_PIXELS: [&str; ${SIZE}] = [
${rows.map((row) => `    "${row}",`).join('\n')}
];
`)
console.log(`rust/src/boot_logo.rs: ${width}x${height} artwork, ${palette.length} colors`)
