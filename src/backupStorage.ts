// Cartridge save data (SRAM/Flash/EEPROM) and save states, kept apart from the ROM library
// so neither database needs a schema migration when the other changes.

const DATABASE_NAME = 'gba-wasm-saves'
// Version 2 added the save-state store; upgrades only create missing stores.
const DATABASE_VERSION = 2
const STORE_NAME = 'backups'
const STATE_STORE_NAME = 'states'
const RECORD_VERSION = 1
export const STATE_SLOTS = [1, 2, 3] as const

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
    const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION)
    request.onupgradeneeded = () => {
      const database = request.result
      if (!database.objectStoreNames.contains(STORE_NAME)) database.createObjectStore(STORE_NAME, { keyPath: 'romHash' })
      if (!database.objectStoreNames.contains(STATE_STORE_NAME)) database.createObjectStore(STATE_STORE_NAME, { keyPath: 'id' })
    }
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

export type StoredState = {
  version: typeof RECORD_VERSION
  id: string
  romHash: string
  slot: number
  /** Core snapshot; the core itself checks its format version and cartridge. */
  data: ArrayBuffer
  /** PNG data URL of the frame shown when the state was saved. */
  screenshot: string
  savedAt: number
}

function isStoredState(value: unknown): value is StoredState {
  if (typeof value !== 'object' || value === null) return false
  const record = value as Partial<StoredState>
  return record.version === RECORD_VERSION
    && typeof record.romHash === 'string'
    && typeof record.slot === 'number'
    && record.data instanceof ArrayBuffer
    && typeof record.screenshot === 'string'
    && typeof record.savedAt === 'number'
}

/** Save states of a ROM by slot; unreadable records are left out. */
export async function loadStates(romHash: string): Promise<Map<number, StoredState>> {
  const database = await openDatabase()
  const records = await new Promise<unknown[]>((resolve, reject) => {
    const store = database.transaction(STATE_STORE_NAME, 'readonly').objectStore(STATE_STORE_NAME)
    const results: unknown[] = []
    let remaining: number = STATE_SLOTS.length
    for (const slot of STATE_SLOTS) {
      const request = store.get(`${romHash}:${slot}`)
      request.onsuccess = () => {
        results.push(request.result)
        remaining -= 1
        if (remaining === 0) resolve(results)
      }
      request.onerror = () => reject(request.error)
    }
  }).finally(() => database.close())
  return new Map(records.filter(isStoredState).map((record) => [record.slot, record]))
}

export async function storeState(romHash: string, slot: number, data: Uint8Array, screenshot: string) {
  const record: StoredState = {
    version: RECORD_VERSION,
    id: `${romHash}:${slot}`,
    romHash,
    slot,
    data: data.slice().buffer,
    screenshot,
    savedAt: Date.now(),
  }
  const database = await openDatabase()
  await new Promise<void>((resolve, reject) => {
    const transaction = database.transaction(STATE_STORE_NAME, 'readwrite')
    transaction.objectStore(STATE_STORE_NAME).put(record)
    transaction.oncomplete = () => resolve()
    transaction.onerror = () => reject(transaction.error)
    transaction.onabort = () => reject(transaction.error ?? new Error('Saving was aborted by the browser.'))
  }).finally(() => database.close())
  return record
}

/** Accepts a `.sav` file when its size fits the cartridge's save memory. */
export function validateSaveFile(backupType: string, currentSize: number, data: Uint8Array) {
  if (backupType === 'none') return 'This cartridge has no save memory.'
  if (backupType === 'EEPROM') {
    return data.length === 512 || data.length === 8192 ? null : `EEPROM saves are 512 or 8,192 bytes; this file is ${data.length.toLocaleString()} bytes.`
  }
  return data.length === currentSize ? null : `This cartridge uses ${currentSize.toLocaleString()}-byte saves; this file is ${data.length.toLocaleString()} bytes.`
}
