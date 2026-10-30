export type SavedGame = {
  id: string
  name: string
  playedAt: number
  screenshot: string
  rom: ArrayBuffer
}

const DATABASE_NAME = 'gba-wasm-library'
const STORE_NAME = 'games'

function openDatabase() {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DATABASE_NAME, 1)
    request.onupgradeneeded = () => request.result.createObjectStore(STORE_NAME, { keyPath: 'id' })
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

export async function getSavedGames(): Promise<SavedGame[]> {
  const database = await openDatabase()
  return new Promise((resolve, reject) => {
    const request = database.transaction(STORE_NAME, 'readonly').objectStore(STORE_NAME).getAll()
    request.onsuccess = () => {
      database.close()
      resolve((request.result as SavedGame[]).sort((a, b) => b.playedAt - a.playedAt))
    }
    request.onerror = () => { database.close(); reject(request.error) }
  })
}

export async function saveGame(game: SavedGame) {
  const database = await openDatabase()
  return new Promise<void>((resolve, reject) => {
    const request = database.transaction(STORE_NAME, 'readwrite').objectStore(STORE_NAME).put(game)
    request.onsuccess = () => { database.close(); resolve() }
    request.onerror = () => { database.close(); reject(request.error) }
  })
}

/** Replaces the library picture of every entry of this game, for example with a save-state frame. */
export async function updateSavedGameScreenshot(name: string, screenshot: string) {
  const database = await openDatabase()
  return new Promise<void>((resolve, reject) => {
    const transaction = database.transaction(STORE_NAME, 'readwrite')
    const store = transaction.objectStore(STORE_NAME)
    const request = store.getAll()
    request.onsuccess = () => {
      for (const game of request.result as SavedGame[]) {
        if (game.name === name) store.put({ ...game, screenshot })
      }
    }
    transaction.oncomplete = () => { database.close(); resolve() }
    transaction.onerror = () => { database.close(); reject(transaction.error) }
    transaction.onabort = () => { database.close(); reject(transaction.error ?? new Error('Updating the library was aborted by the browser.')) }
  })
}

export async function deleteSavedGame(id: string) {
  const database = await openDatabase()
  return new Promise<void>((resolve, reject) => {
    const request = database.transaction(STORE_NAME, 'readwrite').objectStore(STORE_NAME).delete(id)
    request.onsuccess = () => { database.close(); resolve() }
    request.onerror = () => { database.close(); reject(request.error) }
  })
}
