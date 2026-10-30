// Downloads the box art of the featured cartridges and stores WebP thumbnails in public/boxart,
// so the home page cover flow works offline. Requires ImageMagick 7 (`magick`) on PATH.
// Usage: pnpm run boxart:fetch
import { spawnSync } from 'node:child_process'
import { mkdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { FEATURED_GAMES, gbaBoxArtUrl } from '../src/featuredGames.ts'

// Twice the largest cover size in CoverFlow.tsx, for high-density screens.
const THUMBNAIL_SIZE = 440
const outputDir = fileURLToPath(new URL('../public/boxart/', import.meta.url))

mkdirSync(outputDir, { recursive: true })
for (const game of FEATURED_GAMES) {
  const response = await fetch(gbaBoxArtUrl(game.boxArtName))
  if (!response.ok) throw new Error(`${game.boxArtName}: HTTP ${response.status}`)
  const output = `${outputDir}${game.id}.webp`
  const result = spawnSync('magick', ['png:-', '-resize', `${THUMBNAIL_SIZE}x${THUMBNAIL_SIZE}>`, '-strip', '-quality', '82', output], {
    input: Buffer.from(await response.arrayBuffer()),
    stdio: ['pipe', 'inherit', 'inherit'],
  })
  if (result.status !== 0) throw new Error(`${game.id}: magick exited with ${result.status ?? result.error}`)
  console.log(`public/boxart/${game.id}.webp`)
}
