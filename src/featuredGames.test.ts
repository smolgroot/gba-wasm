import { describe, expect, it } from 'vitest'
import { FEATURED_GAMES, buildShowcase, displayGameTitle, normalizeGameTitle } from './featuredGames'
import type { SavedGame } from './savedGames'

function savedGame(name: string, playedAt = 0): SavedGame {
  return { id: name + playedAt, name, playedAt, screenshot: '', rom: new ArrayBuffer(0) }
}

describe('normalizeGameTitle', () => {
  it('ignores region, language and revision tags', () => {
    expect(normalizeGameTitle('Super Mario Advance 4 - Super Mario Bros. 3 (Europe) (En,Fr,De,Es,It) (Rev 1)'))
      .toBe(normalizeGameTitle('Super Mario Advance 4 - Super Mario Bros. 3 (USA)'))
  })
})

describe('displayGameTitle', () => {
  it('drops region tags', () => {
    expect(displayGameTitle('Golden Sun - The Lost Age (USA, Europe) [!]')).toBe('Golden Sun - The Lost Age')
  })
})

describe('buildShowcase', () => {
  it('puts played games first, newest first, then the featured cartridges never played', () => {
    const showcase = buildShowcase(FEATURED_GAMES, [savedGame('Golden Sun (France)', 2), savedGame('Pokemon - Version Rubis (France)', 1)])
    expect(showcase.map((item) => item.id)).toEqual(['golden-sun', 'pokemon-ruby', ...FEATURED_GAMES.map((game) => game.id).filter((id) => id !== 'golden-sun' && id !== 'pokemon-ruby')])
    expect(showcase.slice(2).every((item) => !item.saved)).toBe(true)
  })

  it('matches a ROM from another region to its featured cartridge', () => {
    const rubis = savedGame('Pokemon - Version Rubis (France)')
    const [item] = buildShowcase(FEATURED_GAMES, [rubis])
    expect(item).toMatchObject({ id: 'pokemon-ruby', title: 'Pokémon Ruby', saved: rubis })
  })

  it('includes played games that are not featured', () => {
    const lostAge = savedGame('Golden Sun - The Lost Age (USA)')
    const [item] = buildShowcase(FEATURED_GAMES, [lostAge])
    expect(item).toEqual({ id: 'saved:goldensunthelostage', title: 'Golden Sun - The Lost Age', featured: undefined, saved: lostAge })
  })

  it('shows one cover per game, from its most recent library entry', () => {
    const recent = savedGame('Golden Sun (France)', 2)
    const showcase = buildShowcase([], [recent, savedGame('Golden Sun (USA, Europe)', 1)])
    expect(showcase).toEqual([{ id: 'saved:goldensun', title: 'Golden Sun', featured: undefined, saved: recent }])
  })
})
