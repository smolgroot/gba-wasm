// Cartridge save data (SRAM/Flash), kept apart from the ROM library so neither
// database needs a schema migration when the other changes.

const DATABASE_NAME = 'gba-wasm-saves'
const STORE_NAME = 'backups'
const RECORD_VERSION = 1

export type StoredBackup = {
  version: typeof RECORD_VERSION
  romHash: string
  backupType: string
  data: ArrayBuffer
  savedAt: number
}

/** Saves are keyed by the ROM contents, so every library entry of a ROM shares one save. */
export async function hashRom(rom: Uint8Array) {
  const digest = await crypto.subtle.digest('SHA-256', rom as Uint8Array<ArrayBuffer>)
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function openDatabase() {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DATABASE_NAME, 1)
    request.onupgradeneeded = () => request.result.createObjectStore(STORE_NAME, { keyPath: 'romHash' })
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function isStoredBackup(value: unknown): value is StoredBackup {
  if (typeof value !== 'object' || value === null) return false
  const record = value as Partial<StoredBackup>
  return record.version === RECORD_VERSION
    && typeof record.romHash === 'string'
    && typeof record.backupType === 'string'
    && record.data instanceof ArrayBuffer
}

/** Returns the stored save for a ROM, or null when there is none or it is unreadable. */
export async function loadBackup(romHash: string, backupType: string): Promise<StoredBackup | null> {
  const database = await openDatabase()
  const record = await new Promise<unknown>((resolve, reject) => {
    const request = database.transaction(STORE_NAME, 'readonly').objectStore(STORE_NAME).get(romHash)
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  }).finally(() => database.close())
  if (record === undefined) return null
  if (!isStoredBackup(record) || record.backupType !== backupType) {
    throw new Error('Stored save data is corrupt or from an incompatible version; it was not loaded.')
  }
  return record
}

export async function storeBackup(romHash: string, backupType: string, data: Uint8Array) {
  const record: StoredBackup = {
    version: RECORD_VERSION,
    romHash,
    backupType,
    data: data.slice().buffer,
    savedAt: Date.now(),
  }
  const database = await openDatabase()
  return new Promise<void>((resolve, reject) => {
    const transaction = database.transaction(STORE_NAME, 'readwrite')
    transaction.objectStore(STORE_NAME).put(record)
    transaction.oncomplete = () => resolve()
    // Quota errors surface here when the browser refuses the write.
    transaction.onerror = () => reject(transaction.error)
    transaction.onabort = () => reject(transaction.error ?? new Error('Saving was aborted by the browser.'))
  }).finally(() => database.close())
}
