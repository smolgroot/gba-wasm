import type { SavedGame } from './savedGames'

const GBA_BOX_ART_URL = 'https://thumbnails.libretro.com/Nintendo%20-%20Game%20Boy%20Advance/Named_Boxarts/'

export function gbaBoxArtUrl(name: string) {
  return `${GBA_BOX_ART_URL}${encodeURIComponent(name)}.png`
}

export type FeaturedGame = {
  id: string
  title: string
  year: number
  /** No-Intro name used to fetch the libretro box art. */
  boxArtName: string
  /** Titles (without region tags) a ROM in the library may carry for this game. */
  aliases: string[]
}

/** Known cartridges shown in the home page cover flow. ROMs are never bundled: they come from the library or a file the player picks. */
export const FEATURED_GAMES: FeaturedGame[] = [
  { id: 'pokemon-ruby', title: 'Pokémon Ruby', year: 2002, boxArtName: 'Pokemon - Ruby Version (USA, Europe)', aliases: ['Pokemon - Ruby Version', 'Pokemon - Version Rubis'] },
  { id: 'golden-sun', title: 'Golden Sun', year: 2001, boxArtName: 'Golden Sun (USA, Europe)', aliases: ['Golden Sun'] },
  { id: 'super-mario-advance-4', title: 'Super Mario Advance 4', year: 2003, boxArtName: 'Super Mario Advance 4 - Super Mario Bros. 3 (USA)', aliases: ['Super Mario Advance 4 - Super Mario Bros. 3'] },
  { id: 'donkey-kong-country', title: 'Donkey Kong Country', year: 2003, boxArtName: 'Donkey Kong Country (USA)', aliases: ['Donkey Kong Country'] },
  { id: 'aladdin', title: 'Aladdin', year: 2002, boxArtName: 'Aladdin (Europe) (En,Fr,De,Es)', aliases: ['Aladdin'] },
  { id: 'zelda-minish-cap', title: 'The Minish Cap', year: 2004, boxArtName: 'Legend of Zelda, The - The Minish Cap (USA)', aliases: ['Legend of Zelda, The - The Minish Cap'] },
  { id: 'metroid-fusion', title: 'Metroid Fusion', year: 2002, boxArtName: 'Metroid Fusion (USA)', aliases: ['Metroid Fusion'] },
  { id: 'castlevania-aria', title: 'Aria of Sorrow', year: 2003, boxArtName: 'Castlevania - Aria of Sorrow (USA)', aliases: ['Castlevania - Aria of Sorrow'] },
  { id: 'mario-kart-super-circuit', title: 'Mario Kart: Super Circuit', year: 2001, boxArtName: 'Mario Kart - Super Circuit (USA)', aliases: ['Mario Kart - Super Circuit'] },
]

/** Drops region/revision tags like "(Europe) (Rev 1)" and punctuation so ROM file names from any region compare equal. */
export function normalizeGameTitle(name: string) {
  return name.replace(/\([^)]*\)|\[[^\]]*\]/g, '').toLowerCase().replace(/[^a-z0-9]/g, '')
}

/** The most recently played library entry for this featured game, if any. Expects `games` newest first, as `getSavedGames` returns them. */
export function findLibraryGame(featured: FeaturedGame, games: SavedGame[]) {
  const titles = new Set(featured.aliases.map(normalizeGameTitle))
  return games.find((game) => titles.has(normalizeGameTitle(game.name)))
}
