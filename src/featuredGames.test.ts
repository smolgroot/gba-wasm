import { describe, expect, it } from 'vitest'
import { FEATURED_GAMES, findLibraryGame, normalizeGameTitle } from './featuredGames'
import type { SavedGame } from './savedGames'

function savedGame(name: string, playedAt = 0): SavedGame {
  return { id: name + playedAt, name, playedAt, screenshot: '', rom: new ArrayBuffer(0) }
}

const featured = (id: string) => FEATURED_GAMES.find((game) => game.id === id)!

describe('normalizeGameTitle', () => {
  it('ignores region, language and revision tags', () => {
    expect(normalizeGameTitle('Super Mario Advance 4 - Super Mario Bros. 3 (Europe) (En,Fr,De,Es,It) (Rev 1)'))
      .toBe(normalizeGameTitle('Super Mario Advance 4 - Super Mario Bros. 3 (USA)'))
  })
})

describe('findLibraryGame', () => {
  it('matches a ROM from another region through its aliases', () => {
    const rubis = savedGame('Pokemon - Version Rubis (France)')
    expect(findLibraryGame(featured('pokemon-ruby'), [savedGame('Golden Sun (France)'), rubis])).toBe(rubis)
  })

  it('returns the first match, the most recently played', () => {
    const recent = savedGame('Golden Sun (France)', 2)
    expect(findLibraryGame(featured('golden-sun'), [recent, savedGame('Golden Sun (USA, Europe)', 1)])).toBe(recent)
  })

  it('does not match a different game sharing a prefix', () => {
    expect(findLibraryGame(featured('golden-sun'), [savedGame('Golden Sun - The Lost Age (USA)')])).toBeUndefined()
  })
})
