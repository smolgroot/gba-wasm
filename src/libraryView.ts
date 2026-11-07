import type { SavedGame } from './savedGames'

export type LibraryFilter = 'all' | 'favorites'
export type LibrarySort = 'recent' | 'name' | 'playTime'

export type LibraryView = { query: string; filter: LibraryFilter; sort: LibrarySort }

/** Games to show, favorites first within the chosen order. `playTimes` is keyed by game name. */
export function arrangeLibrary(games: SavedGame[], view: LibraryView, playTimes: Record<string, number>): SavedGame[] {
  const query = view.query.trim().toLowerCase()
  const compare: Record<LibrarySort, (a: SavedGame, b: SavedGame) => number> = {
    recent: (a, b) => b.playedAt - a.playedAt,
    name: (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base', numeric: true }),
    playTime: (a, b) => (playTimes[b.name] ?? 0) - (playTimes[a.name] ?? 0) || b.playedAt - a.playedAt,
  }
  return games
    .filter((game) => (view.filter === 'all' || game.favorite) && game.name.toLowerCase().includes(query))
    .sort((a, b) => Number(Boolean(b.favorite)) - Number(Boolean(a.favorite)) || compare[view.sort](a, b))
}
