import { describe, expect, it } from 'vitest'
import { arrangeLibrary } from './libraryView'
import type { SavedGame } from './savedGames'

function game(name: string, playedAt: number, favorite?: boolean): SavedGame {
  return { id: name, name, playedAt, screenshot: '', rom: new ArrayBuffer(0), favorite }
}

const games = [game('Metroid Fusion', 3), game('Aladdin', 1, true), game('Golden Sun 2', 2), game('Golden Sun 10', 4, true)]
const names = (list: SavedGame[]) => list.map((entry) => entry.name)
const all = { query: '', filter: 'all', sort: 'recent' } as const

describe('arrangeLibrary', () => {
  it('lists favorites first, then the rest, newest first', () => {
    expect(names(arrangeLibrary(games, all, {}))).toEqual(['Golden Sun 10', 'Aladdin', 'Metroid Fusion', 'Golden Sun 2'])
  })

  it('sorts by name with numbers in natural order', () => {
    expect(names(arrangeLibrary(games, { ...all, sort: 'name' }, {}))).toEqual(['Aladdin', 'Golden Sun 10', 'Golden Sun 2', 'Metroid Fusion'])
  })

  it('sorts by play time, most played first', () => {
    const times = { 'Golden Sun 2': 500, 'Metroid Fusion': 900 }
    expect(names(arrangeLibrary(games, { ...all, sort: 'playTime' }, times))).toEqual(['Golden Sun 10', 'Aladdin', 'Metroid Fusion', 'Golden Sun 2'])
  })

  it('filters to favorites and by search text', () => {
    expect(names(arrangeLibrary(games, { ...all, filter: 'favorites' }, {}))).toEqual(['Golden Sun 10', 'Aladdin'])
    expect(names(arrangeLibrary(games, { ...all, query: '  golden ' }, {}))).toEqual(['Golden Sun 10', 'Golden Sun 2'])
  })

  it('does not reorder the input', () => {
    const copy = [...games]
    arrangeLibrary(games, all, {})
    expect(games).toEqual(copy)
  })
})
