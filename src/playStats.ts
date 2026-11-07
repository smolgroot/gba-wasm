// Play time lives in localStorage, not IndexedDB: it is tiny, and a synchronous write survives
// the tab closing, which an asynchronous IndexedDB write may not.
// Keyed by game name like the library pictures, so every library entry of a game shares one total.
const PLAY_TIME_STORAGE_KEY = 'pecten.playTime'

function readAll(): Record<string, number> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(PLAY_TIME_STORAGE_KEY) ?? '{}')
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return {}
    const times: Record<string, number> = {}
    for (const [name, value] of Object.entries(parsed)) {
      if (typeof value === 'number' && Number.isFinite(value) && value > 0) times[name] = value
    }
    return times
  } catch {
    return {}
  }
}

/** Milliseconds played of this game on this device. */
export function getPlayTime(name: string): number {
  return readAll()[name] ?? 0
}

export function getAllPlayTimes(): Record<string, number> {
  return readAll()
}

/** Adds played time and returns the new total. A failed write (storage full or disabled) is ignored. */
export function addPlayTime(name: string, milliseconds: number): number {
  if (!(milliseconds > 0)) return getPlayTime(name)
  const times = readAll()
  times[name] = (times[name] ?? 0) + milliseconds
  try {
    localStorage.setItem(PLAY_TIME_STORAGE_KEY, JSON.stringify(times))
  } catch (error) {
    console.warn('Could not store play time', error)
  }
  return times[name]
}

/** "Not played yet", "< 1 min", "42 min", "3 h 05 min". */
export function formatPlayTime(milliseconds: number): string {
  const minutes = Math.floor(milliseconds / 60_000)
  if (milliseconds <= 0) return 'Not played yet'
  if (minutes < 1) return '< 1 min'
  if (minutes < 60) return `${minutes} min`
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, '0')} min`
}
