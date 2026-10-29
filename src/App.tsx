import { useCallback, useEffect, useRef, useState, type ChangeEvent } from 'react'
import { ArrowDownToLine, ArrowLeft, BookOpen, ChevronRight, CodeXml, Download, Gamepad2, HardDrive, Info, Pause, Play, Rabbit, Save, Settings, Upload, Trash2, Turtle, Volume2, VolumeX, X, type LucideProps } from 'lucide-react'
import { Link, NavLink, Route, Routes, useNavigate } from 'react-router-dom'
import { loadGbaWasm, type GbaWasmEmulator } from './wasmBridge'
import { deleteSavedGame, getSavedGames, saveGame, type SavedGame } from './savedGames'
import { hashRom, loadBackup, loadStates, STATE_SLOTS, storeBackup, storeState, validateSaveFile, type StoredState } from './backupStorage'
import { attachKeyboard } from './keypad'
import { TouchControls } from './TouchControls'
import { TOUCH_THEMES, isTouchTheme, type TouchTheme } from './touchThemes'
import { AudioOutput } from './audio'

type ActiveGame = { launchId: number; name: string; status: string; pixels: Uint8ClampedArray; emulator: GbaWasmEmulator; rom: Uint8Array; romHash: string; backupType: string }
type Toast = { id: number; message: string; tone: 'info' | 'error' }
const TOAST_DURATION_MS = 2500
type SaveFlushRef = { current: (() => void) | null }
const DOCS_URL = '/gba-wasm/docs/'
const SOURCE_URL = 'https://github.com/smolgroot/gba-wasm'
const GBA_BOX_ART_URL = 'https://thumbnails.libretro.com/Nintendo%20-%20Game%20Boy%20Advance/Named_Boxarts/'
const GBA_FRAME_DURATION_MS = (1000 * 280_896) / 16_777_216
// Display refresh timestamps jitter; a frame due within this margin runs now.
const FRAME_PACING_TOLERANCE_MS = 2
// Upper bound on frames run per display refresh, so fast mode on a slow display cannot stall input.
const MAX_FRAMES_PER_CALLBACK = 4
const STARTUP_SPLASH_DURATION_MS = 1800
const SETTINGS_STORAGE_KEY = 'gba-wasm.settings'
const KEYBOARD_CONTROLS = [['←↑↓→', 'D-PAD'], ['X', 'A'], ['Z', 'B'], ['A', 'L'], ['S', 'R'], ['ENTER', 'START'], ['SHIFT', 'SELECT'], ['P', 'PAUSE']]
// Games write saves over several frames; persist once writes have stopped for this long.
const SAVE_QUIET_FRAMES = 30
// Keep in sync with the touch overlay media query in App.css.
const TOUCH_LANDSCAPE_QUERY = '(orientation: landscape) and (pointer: coarse) and (max-height: 540px)'

type GameSpeed = 'slow' | 'normal' | 'fast'

const GAME_SPEED_MULTIPLIERS: Record<GameSpeed, number> = { slow: 0.5, normal: 1, fast: 2 }

type EmulatorSettings = {
  skipStartupSplash: boolean
  sound: boolean
  /** Output gain from 0 to 1. */
  volume: number
  speed: GameSpeed
  touchControls: boolean
  touchTheme: TouchTheme
}

function isGameSpeed(value: unknown): value is GameSpeed {
  return typeof value === 'string' && value in GAME_SPEED_MULTIPLIERS
}

// Touch controls default to on for phones and tablets, where there is usually no keyboard.
const DEFAULT_SETTINGS: EmulatorSettings = { skipStartupSplash: false, sound: true, volume: 0.8, speed: 'normal', touchControls: typeof matchMedia === 'function' && matchMedia('(pointer: coarse)').matches, touchTheme: 'gba-wasm' }

/** A running figure in the lucide icon style (lucide has no running-person icon). */
function RunnerIcon({ size = 24, ...props }: LucideProps) {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" {...props}>
      <circle cx="14.5" cy="4" r="2" />
      <path d="M13 7.5 10 13l3.5 2.5L12 21" />
      <path d="m10 13-3 3H4" />
      <path d="M7 9.5 10.5 7l3 2.5 3 1" />
    </svg>
  )
}

const GAME_SPEED_OPTIONS: { speed: GameSpeed; label: string; Icon: (props: LucideProps) => React.ReactNode }[] = [
  { speed: 'slow', label: 'Slow', Icon: Turtle },
  { speed: 'normal', label: 'Normal', Icon: RunnerIcon },
  { speed: 'fast', label: 'Fast', Icon: Rabbit },
]

function readSettings(): EmulatorSettings {
  try {
    const stored = localStorage.getItem(SETTINGS_STORAGE_KEY)
    if (!stored) return DEFAULT_SETTINGS
    const parsed: unknown = JSON.parse(stored)
    if (typeof parsed === 'object' && parsed !== null) {
      const values = parsed as Partial<Record<keyof EmulatorSettings, unknown>>
      return {
        skipStartupSplash: 'skipStartupSplash' in values ? Boolean(values.skipStartupSplash) : DEFAULT_SETTINGS.skipStartupSplash,
        sound: 'sound' in values ? Boolean(values.sound) : DEFAULT_SETTINGS.sound,
        volume: typeof values.volume === 'number' && Number.isFinite(values.volume) ? Math.min(1, Math.max(0, values.volume)) : DEFAULT_SETTINGS.volume,
        speed: isGameSpeed(values.speed) ? values.speed : DEFAULT_SETTINGS.speed,
        touchControls: 'touchControls' in values ? Boolean(values.touchControls) : DEFAULT_SETTINGS.touchControls,
        touchTheme: isTouchTheme(values.touchTheme) ? values.touchTheme : DEFAULT_SETTINGS.touchTheme,
      }
    }
  } catch {
    // Use defaults when storage is unavailable or malformed.
  }
  return DEFAULT_SETTINGS
}

function formatSavedDate(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(timestamp)
}

function fileToImage(pixels: Uint8ClampedArray) {
  const canvas = document.createElement('canvas')
  canvas.width = 240
  canvas.height = 160
  const context = canvas.getContext('2d')
  if (!context) return ''
  context.putImageData(new ImageData(new Uint8ClampedArray(pixels), 240, 160), 0, 0)
  return canvas.toDataURL('image/png')
}

function hasVisibleVideo(pixels: Uint8ClampedArray) {
  for (let index = 0; index < pixels.length; index += 4) {
    if (pixels[index] !== 0 || pixels[index + 1] !== 0 || pixels[index + 2] !== 0) return true
  }
  return false
}

function describeError(error: unknown) {
  if (error instanceof Error) return { message: error.message, stack: error.stack }
  if (typeof error === 'string') return { message: error, stack: undefined }
  try {
    return { message: JSON.stringify(error), stack: undefined }
  } catch {
    return { message: String(error), stack: undefined }
  }
}

function gbaBoxArtUrl(name: string) {
  return `${GBA_BOX_ART_URL}${encodeURIComponent(name)}.png`
}

function GithubMark() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width="19" height="19" fill="currentColor">
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.726-4.043-1.61-4.043-1.61-.546-1.387-1.333-1.756-1.333-1.756-1.09-.745.083-.73.083-.73 1.205.085 1.84 1.237 1.84 1.237 1.07 1.834 2.807 1.304 3.492.997.108-.775.42-1.305.762-1.605-2.665-.303-5.467-1.334-5.467-5.931 0-1.31.468-2.381 1.236-3.221-.124-.303-.536-1.524.117-3.176 0 0 1.008-.322 3.301 1.23a11.5 11.5 0 0 1 6.009 0c2.292-1.552 3.298-1.23 3.298-1.23.655 1.652.243 2.873.12 3.176.77.84 1.235 1.911 1.235 3.221 0 4.61-2.807 5.625-5.48 5.921.431.372.815 1.103.815 2.222 0 1.604-.015 2.896-.015 3.29 0 .322.216.694.825.576C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  )
}

function GameBoxArt({ name, fallback, alt, className }: { name: string; fallback: string; alt: string; className?: string }) {
  const [showFallback, setShowFallback] = useState(false)
  return <img className={className} src={showFallback ? fallback || `${import.meta.env.BASE_URL}logo.png` : gbaBoxArtUrl(name)} alt={alt} onError={() => setShowFallback(true)} />
}

function CartridgeMockup({ name }: { name: string }) {
  const [showBoxArt, setShowBoxArt] = useState(true)

  return (
    <div className="cartridge-mockup" aria-label={`Cartridge: ${name}`}>
      <div className="cartridge-grip" />
      <div className="cartridge-label">
        {showBoxArt ? (
          <img src={gbaBoxArtUrl(name)} alt={`${name} box art`} onError={() => setShowBoxArt(false)} />
        ) : <><span>CARTRIDGE</span><strong>{name}</strong><small>GAME BOY ADVANCE</small></>}
      </div>
      <div className="cartridge-contacts" aria-hidden="true"><i /><i /><i /><i /><i /><i /><i /></div>
    </div>
  )
}

function App() {
  const inputRef = useRef<HTMLInputElement | null>(null)
  const emulatorRef = useRef<GbaWasmEmulator | null>(null)
  // Replaced emulators stay alive until the page that drives them has unmounted, so its frame
  // loop and save tracking can never call into freed WebAssembly memory.
  const retiredEmulatorsRef = useRef<GbaWasmEmulator[]>([])
  const flushSaveRef = useRef<(() => void) | null>(null)
  const launchIdRef = useRef(0)
  const navigate = useNavigate()
  const [activeGame, setActiveGame] = useState<ActiveGame | null>(null)
  const [savedGames, setSavedGames] = useState<SavedGame[]>([])
  const [appStatus, setAppStatus] = useState('Initializing WebAssembly')
  const [romLoadLog, setRomLoadLog] = useState<string | null>(null)
  const [settings, setSettings] = useState<EmulatorSettings>(readSettings)

  function updateSettings(nextSettings: EmulatorSettings) {
    setSettings(nextSettings)
    try {
      localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(nextSettings))
    } catch {
      // The setting remains active for this session if storage is unavailable.
    }
  }

  useEffect(() => {
    let mounted = true
    void loadGbaWasm().then((wasm) => {
      if (mounted) setAppStatus(wasm.hello('GBA'))
    }).catch((error: unknown) => {
      if (mounted) setAppStatus(error instanceof Error ? error.message : 'WebAssembly failed to load')
    })
    void getSavedGames().then((games) => {
      if (mounted) setSavedGames(games)
    })
    const flushSave = flushSaveRef
    const retiredEmulators = retiredEmulatorsRef
    return () => {
      mounted = false
      flushSave.current?.()
      emulatorRef.current?.free()
      emulatorRef.current = null
      for (const emulator of retiredEmulators.current.splice(0)) emulator.free()
    }
  }, [])

  async function executeRom(rom: Uint8Array, name: string) {
    const wasm = await loadGbaWasm()
    const info = wasm.inspect_rom(rom)
    const emulator = wasm.create_emulator(rom)
    let pixels: Uint8ClampedArray
    let romHash: string
    let saveStatus = ''
    const backupType = emulator.backup_type()
    try {
      romHash = await hashRom(rom)
      // EEPROM size is only known once the game runs, so check the type, not the data.
      if (backupType !== 'none') {
        try {
          const stored = await loadBackup(romHash, backupType)
          if (stored) {
            emulator.load_backup(new Uint8Array(stored.data))
            saveStatus = `, ${backupType} save loaded`
          }
        } catch (error) {
          saveStatus = `, ${describeError(error).message}`
        }
      }
      pixels = emulator.step_frame()
    } catch (error) {
      emulator.free()
      throw error
    }
    flushSaveRef.current?.()
    if (emulatorRef.current) retiredEmulatorsRef.current.push(emulatorRef.current)
    emulatorRef.current = emulator
    const status = `${info}: ${hasVisibleVideo(pixels) ? 'running' : 'no video output yet'}${saveStatus}`
    setActiveGame({ launchId: ++launchIdRef.current, name, status, pixels, emulator, rom, romHash, backupType })
    setAppStatus(status)
    return pixels
  }

  // Runs after the commit that mounts the new game, when the old page's cleanups have run.
  useEffect(() => {
    for (const emulator of retiredEmulatorsRef.current.splice(0)) emulator.free()
  }, [activeGame])

  async function loadRomFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.currentTarget.files?.[0]
    event.currentTarget.value = ''
    if (!file) return
    let stage = 'reading file'
    let romSize = 0
    try {
      const rom = new Uint8Array(await file.arrayBuffer())
      romSize = rom.byteLength
      const name = file.name.replace(/\.(gba|bin)$/i, '')
      stage = 'validating and executing ROM'
      setRomLoadLog(null)
      const pixels = await executeRom(rom, name)
      stage = 'saving game to local library'
      const game: SavedGame = {
        id: crypto.randomUUID(),
        name,
        playedAt: Date.now(),
        screenshot: hasVisibleVideo(pixels) ? fileToImage(pixels) : '',
        rom: rom.buffer.slice(rom.byteOffset, rom.byteOffset + rom.byteLength) as ArrayBuffer,
      }
      await saveGame(game)
      setSavedGames(await getSavedGames())
      setRomLoadLog(null)
      navigate('/play')
    } catch (error) {
      const detail = describeError(error)
      const message = detail.message || 'Unknown error'
      const log = [
        `[ROM LOAD] ${new Date().toISOString()}`,
        `file: ${file.name}`,
        `size: ${romSize || file.size} bytes`,
        `stage: ${stage}`,
        `error: ${message}`,
        ...(detail.stack ? [`stack: ${detail.stack}`] : []),
      ].join('\n')
      console.error('Could not load this ROM', error)
      setRomLoadLog(log)
      setAppStatus(message)
    }
  }

  /** Boots the running cartridge again, for example so it reads an imported save. */
  async function restartGame(game: ActiveGame, message: string) {
    try {
      await executeRom(game.rom, game.name)
      setAppStatus(message)
    } catch (error) {
      setAppStatus(describeError(error).message)
    }
  }

  async function resumeGame(game: SavedGame) {
    try {
      await executeRom(new Uint8Array(game.rom), game.name)
      navigate('/play')
    } catch (error) {
      setAppStatus(error instanceof Error ? error.message : 'Could not resume this game')
    }
  }

  async function removeGame(id: string) {
    await deleteSavedGame(id)
    setSavedGames(await getSavedGames())
  }

  const triggerLoad = () => {
    navigate('/play')
    inputRef.current?.click()
  }
  return (
    <div className="app-shell">
      <header className="site-header">
        <Link className="brand" to="/" aria-label="GBA-WASM home">
          <img src={`${import.meta.env.BASE_URL}logo.png`} alt="" /><span>GBA<span className="brand-dash">-</span>WASM</span>
        </Link>
        <nav className="header-nav" aria-label="Main navigation">
          <NavLink to="/saved-games" className={({ isActive }) => isActive ? 'nav-link active' : 'nav-link'}>
            Saved games <span className="saved-count">{savedGames.length}</span>
          </NavLink>
          <a className="nav-icon-link" href={DOCS_URL} aria-label="Documentation" title="Documentation"><BookOpen size={18} /></a>
          <a className="nav-icon-link" href={SOURCE_URL} target="_blank" rel="noreferrer" aria-label="Star GBA-WASM on GitHub" title="Star GBA-WASM on GitHub"><GithubMark /></a>
          <button className="header-load" onClick={triggerLoad}><ArrowDownToLine size={17} /><span>Load ROM</span></button>
        </nav>
        <input ref={inputRef} className="file-input" type="file" accept=".gba,.bin,application/octet-stream" onChange={(event) => void loadRomFile(event)} />
      </header>
      <main>
        <Routes>
          <Route path="/" element={<HomePage savedGames={savedGames} onLoad={triggerLoad} onResume={resumeGame} />} />
          <Route path="/play" element={<EmulatorPage key={activeGame?.launchId ?? 'empty'} game={activeGame} status={appStatus} errorLog={romLoadLog} settings={settings} onSettingsChange={updateSettings} onLoad={triggerLoad} onRuntimeError={setAppStatus} flushSaveRef={flushSaveRef} onRestart={restartGame} />} />
          <Route path="/saved-games" element={<SavedGamesPage games={savedGames} onResume={resumeGame} onRemove={removeGame} />} />
          <Route path="*" element={<HomePage savedGames={savedGames} onLoad={triggerLoad} onResume={resumeGame} />} />
        </Routes>
      </main>
      <footer className="site-footer">
        <span>GBA-WASM <span className="footer-dot">/</span> Rust + WebAssembly</span>
        <span className="runtime-status"><i /> {appStatus}</span>
        <p className="legal-disclaimer">GBA-WASM is an open-source Game Boy Advance emulator. This project is not affiliated with Nintendo. Game Boy Advance is a registered trademark of Nintendo Co., Ltd.</p>
      </footer>
    </div>
  )
}

function HomePage({ savedGames, onLoad, onResume }: { savedGames: SavedGame[]; onLoad: () => void; onResume: (game: SavedGame) => void }) {
  return (
    <div className="home-page">
      <section className="hero-section">
        <div className="hero-copy">
          <h1>Your games.<br /><span>In your browser.</span></h1>
          <p className="hero-description">A Game Boy Advance playground powered by Rust and WebAssembly. Bring a ROM and pick up where you left off.</p>
          <div className="hero-actions">
            <button className="primary-button" onClick={onLoad}><ArrowDownToLine size={18} /> Load ROM</button>
            <Link className="secondary-button" to="/saved-games"><HardDrive size={17} /> Saved games <ChevronRight size={16} /></Link>
          </div>
          <div className="hero-links">
            <a href={DOCS_URL}><BookOpen size={15} /> Documentation <ArrowLeft className="link-arrow" size={13} /></a>
            <a href={SOURCE_URL} target="_blank" rel="noreferrer"><CodeXml size={15} /> Source code <ArrowLeft className="link-arrow" size={13} /></a>
          </div>
        </div>
        <div className="console-stage" aria-label="GBA-WASM console illustration">
          <div className="console">
            <div className="console-topline"><span>GBA-WASM</span><span>ADVANCE</span></div>
            <div className="console-body">
              <div className="console-left-controls">
                <div className="dpad"><span /><i /><b /><em /><strong /></div>
                <div className="system-buttons"><b>SELECT</b><b>START</b></div>
              </div>
              <div className="console-screen-frame"><div className="console-screen"><div className="screen-scanlines" /><img src={`${import.meta.env.BASE_URL}logo.png`} alt="GBA-WASM" /><span>PRESS START</span></div></div>
              <div className="console-right-controls">
                <div className="action-buttons"><b>B</b><b>A</b></div>
                <div className="console-speaker"><i /><i /><i /><i /><i /><i /></div>
              </div>
            </div>
          </div>
        </div>
      </section>
      <section className="library-strip">
        <div className="section-heading"><div><p className="eyebrow">YOUR LIBRARY</p><h2>Recently played</h2></div><Link to="/saved-games" className="text-link">View library <ChevronRight size={16} /></Link></div>
        {savedGames.length ? (
          <div className="recent-grid">{savedGames.slice(0, 3).map((game) => (
            <button className="recent-game" key={game.id} onClick={() => onResume(game)}>
              <GameBoxArt key={game.name} name={game.name} fallback={game.screenshot} alt={`${game.name} artwork`} />
              <span><strong>{game.name}</strong><small>{formatSavedDate(game.playedAt)}</small></span><ChevronRight size={17} />
            </button>
          ))}</div>
        ) : <div className="empty-library"><Gamepad2 size={20} /><span>Your library is ready for its first game.</span></div>}
      </section>
    </div>
  )
}

function EmulatorPage({ game, status, errorLog, settings, onSettingsChange, onLoad, onRuntimeError, flushSaveRef, onRestart }: { game: ActiveGame | null; status: string; errorLog: string | null; settings: EmulatorSettings; onSettingsChange: (settings: EmulatorSettings) => void; onLoad: () => void; onRuntimeError: (status: string) => void; flushSaveRef: SaveFlushRef; onRestart: (game: ActiveGame, message: string) => Promise<void> }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const playSurfaceRef = useRef<HTMLDivElement | null>(null)
  // The frame loop reads the ref, so pausing does not restart (and repaint) the loop.
  const pausedRef = useRef(false)
  const audioRef = useRef<AudioOutput | null>(null)
  // Read by the frame loop and the audio setup, so changing them restarts neither.
  const speedRef = useRef(settings.speed)
  const volumeRef = useRef(settings.volume)
  const [paused, setPaused] = useState(false)
  const blankFrameCountRef = useRef(0)
  const [hasVideo, setHasVideo] = useState(() => Boolean(game && hasVisibleVideo(game.pixels)))
  const [noVideoOutput, setNoVideoOutput] = useState(false)
  const [runtimeError, setRuntimeError] = useState<string | null>(null)
  const [showSettings, setShowSettings] = useState(false)
  const [showSaves, setShowSaves] = useState(false)
  const [stateSlots, setStateSlots] = useState<Map<number, StoredState>>(new Map())
  const [pendingImport, setPendingImport] = useState<{ name: string; data: Uint8Array } | null>(null)
  const [importError, setImportError] = useState<string | null>(null)
  const [toast, setToast] = useState<Toast | null>(null)
  const importInputRef = useRef<HTMLInputElement | null>(null)
  const showToast = useCallback((message: string, tone: Toast['tone'] = 'info') => setToast({ id: Date.now(), message, tone }), [])

  useEffect(() => {
    if (!toast) return
    const timer = window.setTimeout(() => setToast(null), TOAST_DURATION_MS)
    return () => window.clearTimeout(timer)
  }, [toast])

  const refreshStates = useCallback(async () => {
    if (!game) return new Map<number, StoredState>()
    const states = await loadStates(game.romHash)
    setStateSlots(states)
    return states
  }, [game])

  function openSaves() {
    setShowSaves(true)
    setImportError(null)
    setPendingImport(null)
    void refreshStates().catch((error: unknown) => showToast(`Could not read save states: ${describeError(error).message}`, 'error'))
  }

  const saveStateToSlot = useCallback(async (slot: number) => {
    if (!game) return
    try {
      const data = game.emulator.save_state()
      const screenshot = canvasRef.current?.toDataURL('image/png') ?? ''
      await storeState(game.romHash, slot, data, screenshot)
      await refreshStates()
      showToast(`State saved to slot ${slot}`)
    } catch (error) {
      showToast(`Could not save state: ${describeError(error).message}`, 'error')
    }
  }, [game, refreshStates, showToast])

  const loadStateFromSlot = useCallback(async (slot: number) => {
    if (!game) return
    try {
      const record = (await refreshStates()).get(slot)
      if (!record) {
        showToast(`Slot ${slot} is empty`, 'error')
        return
      }
      game.emulator.load_state(new Uint8Array(record.data))
      // Drop audio generated before the load and show the restored frame, even while paused.
      game.emulator.take_audio_samples()
      canvasRef.current?.getContext('2d')?.putImageData(new ImageData(new Uint8ClampedArray(game.emulator.frame()), 240, 160), 0, 0)
      showToast(`State loaded from slot ${slot}`)
    } catch (error) {
      showToast(`Could not load state: ${describeError(error).message}`, 'error')
    }
  }, [game, refreshStates, showToast])

  useEffect(() => {
    if (!game) return
    // F2 quick-saves and F4 quick-loads slot 1.
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.code !== 'F2' && event.code !== 'F4') || event.repeat) return
      event.preventDefault()
      void (event.code === 'F2' ? saveStateToSlot(1) : loadStateFromSlot(1))
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [game, saveStateToSlot, loadStateFromSlot])

  function downloadSaveFile() {
    if (!game) return
    const data = game.emulator.backup_data()
    const url = URL.createObjectURL(new Blob([data.slice().buffer], { type: 'application/octet-stream' }))
    const link = document.createElement('a')
    link.href = url
    link.download = `${game.name}.sav`
    link.click()
    URL.revokeObjectURL(url)
  }

  async function chooseSaveFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.currentTarget.files?.[0]
    event.currentTarget.value = ''
    if (!file || !game) return
    const data = new Uint8Array(await file.arrayBuffer())
    const problem = validateSaveFile(game.backupType, game.emulator.backup_data().length, data)
    setImportError(problem)
    setPendingImport(problem ? null : { name: file.name, data })
  }

  async function importSaveFile() {
    if (!game || !pendingImport) return
    try {
      // Persist (and stop tracking) the current save first, so it cannot overwrite the import.
      flushSaveRef.current?.()
      await storeBackup(game.romHash, game.backupType, pendingImport.data)
      await onRestart(game, `Imported ${pendingImport.name}; the game restarted with it`)
    } catch (error) {
      setImportError(`Could not import: ${describeError(error).message}`)
    }
  }
  const [showDetails, setShowDetails] = useState(false)
  const [showStartupSplash, setShowStartupSplash] = useState(() => !settings.skipStartupSplash)
  const splashVisible = showStartupSplash && !settings.skipStartupSplash

  function togglePause() {
    pausedRef.current = !pausedRef.current
    setPaused(pausedRef.current)
  }

  useEffect(() => {
    if (!game) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.code !== 'KeyP' || event.repeat || event.ctrlKey || event.metaKey || event.altKey) return
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return
      pausedRef.current = !pausedRef.current
      setPaused(pausedRef.current)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [game])

  useEffect(() => {
    if (!splashVisible) return
    const timer = window.setTimeout(() => setShowStartupSplash(false), STARTUP_SPLASH_DURATION_MS)
    return () => window.clearTimeout(timer)
  }, [splashVisible])

  // Keyboard and touch input are tracked separately and combined, so neither releases the other's buttons.
  const keyboardKeysRef = useRef(0)
  const touchKeysRef = useRef(0)
  const sendKeys = useCallback(() => game?.emulator.set_pressed_keys(keyboardKeysRef.current | touchKeysRef.current), [game])
  const onTouchKeys = useCallback((pressed: number) => {
    touchKeysRef.current = pressed
    sendKeys()
  }, [sendKeys])

  // In phone landscape the touch overlay fills the viewport, so bring it into view on rotation.
  useEffect(() => {
    if (!game || !settings.touchControls) return
    const landscape = matchMedia(TOUCH_LANDSCAPE_QUERY)
    const reveal = () => { if (landscape.matches) playSurfaceRef.current?.scrollIntoView({ block: 'start' }) }
    reveal()
    landscape.addEventListener('change', reveal)
    return () => landscape.removeEventListener('change', reveal)
  }, [game, settings.touchControls])

  useEffect(() => {
    if (!game) return
    return attachKeyboard(window, (pressed) => {
      keyboardKeysRef.current = pressed
      sendKeys()
    })
  }, [game, sendKeys])

  useEffect(() => {
    speedRef.current = settings.speed
    audioRef.current?.setPlaybackRate(GAME_SPEED_MULTIPLIERS[settings.speed])
  }, [settings.speed])

  useEffect(() => {
    volumeRef.current = settings.volume
    audioRef.current?.setVolume(settings.volume)
  }, [settings.volume])

  useEffect(() => {
    if (!game || !settings.sound) return
    let active = true
    let output: AudioOutput | null = null
    void AudioOutput.create(game.emulator.audio_sample_rate(), volumeRef.current).then((created) => {
      if (!active) return created.close()
      output = created
      audioRef.current = created
      created.setPlaybackRate(GAME_SPEED_MULTIPLIERS[speedRef.current])
      created.resume()
    }).catch((error: unknown) => {
      if (active) onRuntimeError(`Audio unavailable: ${describeError(error).message}`)
    })
    // Browsers only start audio after a user gesture.
    const resume = () => output?.resume()
    window.addEventListener('keydown', resume)
    window.addEventListener('pointerdown', resume)
    return () => {
      active = false
      window.removeEventListener('keydown', resume)
      window.removeEventListener('pointerdown', resume)
      if (audioRef.current === output) audioRef.current = null
      output?.close()
    }
  }, [game, settings.sound, onRuntimeError])

  useEffect(() => {
    if (!game || game.backupType === 'none') return
    let savePending = false
    let quietFrames = 0
    const flushSave = () => {
      if (!savePending) return
      savePending = false
      const data = game.emulator.backup_data()
      void storeBackup(game.romHash, game.backupType, data).then(() => showToast('Game saved')).catch((error: unknown) => {
        showToast(`Could not store save data: ${describeError(error).message}`, 'error')
        onRuntimeError(`Could not store save data: ${describeError(error).message}`)
      })
    }
    const trackSaveWrites = () => {
      if (game.emulator.take_backup_dirty()) {
        savePending = true
        quietFrames = 0
      } else if (savePending && ++quietFrames >= SAVE_QUIET_FRAMES) {
        flushSave()
      }
    }
    const interval = window.setInterval(trackSaveWrites, GBA_FRAME_DURATION_MS)
    const onPageHide = () => {
      trackSaveWrites()
      flushSave()
    }
    // Final flush; afterwards nothing touches this emulator, so App may free it.
    const close = () => {
      window.clearInterval(interval)
      window.removeEventListener('pagehide', onPageHide)
      onPageHide()
      if (flushSaveRef.current === close) flushSaveRef.current = null
    }
    flushSaveRef.current = close
    window.addEventListener('pagehide', onPageHide)
    return () => {
      if (flushSaveRef.current === close) close()
    }
  }, [game, flushSaveRef, onRuntimeError, showToast])

  useEffect(() => {
    const context = canvasRef.current?.getContext('2d')
    if (!game || !context) return

    context.putImageData(new ImageData(new Uint8ClampedArray(game.pixels), 240, 160), 0, 0)
    let animationFrame = 0
    let running = true
    let nextFrameAt: number | null = null
    const advanceFrame = (timestamp: number) => {
      if (!running) return
      if (pausedRef.current) {
        // Resume on the next animation frame instead of catching up on paused time.
        nextFrameAt = null
      } else {
        const speed = speedRef.current
        const frameDuration = GBA_FRAME_DURATION_MS / GAME_SPEED_MULTIPLIERS[speed]
        nextFrameAt ??= timestamp
        let pixels: Uint8ClampedArray | null = null
        try {
          // Each frame advances the deadline by exactly one (speed-scaled) GBA frame, so the
          // average rate is 59.73 fps times the speed; fast mode runs several frames per refresh.
          for (let frames = 0; frames < MAX_FRAMES_PER_CALLBACK && timestamp >= nextFrameAt - FRAME_PACING_TOLERANCE_MS; frames += 1) {
            pixels = game.emulator.step_frame()
            // Always drain the core's buffer; the output plays it back at the game speed.
            audioRef.current?.push(game.emulator.take_audio_samples())
            nextFrameAt += frameDuration
          }
          // After a stall (background tab, slow device), resynchronize instead of catching up.
          if (timestamp - nextFrameAt > frameDuration * 3) nextFrameAt = timestamp + frameDuration
        } catch (error) {
          running = false
          const message = error instanceof Error ? error.message : String(error)
          setRuntimeError(message)
          onRuntimeError(message)
          return
        }
        if (pixels) {
          context.putImageData(new ImageData(new Uint8ClampedArray(pixels), 240, 160), 0, 0)
          if (hasVisibleVideo(pixels)) {
            blankFrameCountRef.current = 0
            setHasVideo(true)
            setNoVideoOutput(false)
          } else {
            blankFrameCountRef.current += 1
            if (blankFrameCountRef.current >= 120) {
              setHasVideo(false)
              setNoVideoOutput(true)
            }
          }
        }
      }
      animationFrame = requestAnimationFrame(advanceFrame)
    }
    animationFrame = requestAnimationFrame(advanceFrame)

    return () => {
      running = false
      cancelAnimationFrame(animationFrame)
    }
  }, [game, onRuntimeError, splashVisible])

  if (!game) return (
    <section className="empty-emulator">
      <div className="empty-emulator-icon"><Gamepad2 size={30} /></div><p className="eyebrow">NO CARTRIDGE INSERTED</p>
      <h1>Ready when you are.</h1><p>Load a compatible ROM to open the emulator.</p>
      <button className="primary-button" onClick={onLoad}><ArrowDownToLine size={18} /> Load ROM</button><small>{status}</small>
      {errorLog && <pre className="rom-error-log" aria-label="ROM load diagnostic log"><code>{errorLog}</code></pre>}
    </section>
  )

  // Shown in the side card on desktop and in the Details dialog on mobile, where the card is hidden.
  const cartridgeDetails = <>
    <CartridgeMockup key={game.name} name={game.name} />
    <p>ROM loaded into the WebAssembly runtime.</p>
    <button className="secondary-button wide-button" onClick={() => { setShowDetails(false); onLoad() }}><ArrowDownToLine size={17} /> Replace ROM</button>
    <div className="keyboard-note"><span>CONTROLS</span><div>{KEYBOARD_CONTROLS.map(([key, button]) => <span className="control" key={button}><kbd>{key}</kbd><small>{button}</small></span>)}</div></div>
  </>

  return (
    <section className="emulator-page">
      <div className="emulator-heading"><div><h1>{game.name}</h1></div><div className="emulator-heading-actions"><button className="secondary-button" onClick={togglePause} disabled={Boolean(runtimeError)} aria-pressed={paused}>{paused ? <Play size={17} /> : <Pause size={17} />} {paused ? 'Resume' : 'Pause'}</button><button className="secondary-button" onClick={() => onSettingsChange({ ...settings, touchControls: !settings.touchControls })} aria-pressed={settings.touchControls} aria-label="Touch controls" title="Touch controls"><Gamepad2 size={17} /> Touch</button><button className="secondary-button mobile-only" onClick={() => setShowDetails(true)} aria-label="Details" title="Details"><Info size={17} /> Details</button><button className="secondary-button" onClick={openSaves}><Save size={17} /> Saves</button><button className="secondary-button" onClick={() => setShowSettings(true)}><Settings size={17} /> Settings</button><Link to="/saved-games" className="secondary-button"><HardDrive size={17} /> Library</Link></div></div>
      <div className="emulator-layout">
        <div className={`play-surface${settings.touchControls ? ' has-touch-controls' : ''}`} ref={playSurfaceRef}>
          <div className="screen-well"><canvas ref={canvasRef} width={240} height={160} />
            {splashVisible && <button className="startup-splash" onClick={() => setShowStartupSplash(false)} aria-label="Skip startup splash"><img src={`${import.meta.env.BASE_URL}logo.png`} alt="" /><strong>GBA<span>-</span>WASM</strong><small>Tap to skip</small></button>}
            {!splashVisible && !runtimeError && paused && <button className="emulation-notice paused-notice" onClick={togglePause}><strong>Paused</strong><span>Press P or click to resume.</span></button>}
            {!splashVisible && runtimeError && <div className="emulation-notice" role="alert"><strong>Emulation stopped</strong><span>{runtimeError}</span></div>}
            {!splashVisible && !runtimeError && noVideoOutput && !hasVideo && <div className="emulation-notice" role="status"><strong>No video output yet</strong><span>This ROM has not reached a display state the current emulator can render. CPU, BIOS, and graphics support are still incomplete.</span></div>}
          </div>
          {settings.touchControls && <TouchControls theme={settings.touchTheme} onChange={onTouchKeys} />}
        </div>
        <aside className="emulator-aside">{cartridgeDetails}</aside>
      </div>
      {/* <p className="emulator-disclaimer">The emulator core is in early development. Audio, EEPROM saves, and many BIOS services are not implemented, and accuracy varies by game.</p> */}
      {/* {showDetails && <div className="settings-backdrop" role="presentation" onMouseDown={() => setShowDetails(false)}>
        <section className="settings-modal details-modal" role="dialog" aria-modal="true" aria-labelledby="details-title" onMouseDown={(event) => event.stopPropagation()}>
          <div className="settings-modal-heading"><div><p className="eyebrow">CARTRIDGE</p><h2 id="details-title">Details</h2></div><button className="modal-close" onClick={() => setShowDetails(false)} aria-label="Close details"><X size={18} /></button></div>
          {cartridgeDetails}
        </section>
      </div>} */}
      {toast && <div key={toast.id} className={`toast toast-${toast.tone}`} role={toast.tone === 'error' ? 'alert' : 'status'}>{toast.message}</div>}
      {showSaves && <div className="settings-backdrop" role="presentation" onMouseDown={() => setShowSaves(false)}>
        <section className="settings-modal saves-modal" role="dialog" aria-modal="true" aria-labelledby="saves-title" onMouseDown={(event) => event.stopPropagation()}>
          <div className="settings-modal-heading"><div><p className="eyebrow">PROGRESS</p><h2 id="saves-title">Saves</h2></div><button className="modal-close" onClick={() => setShowSaves(false)} aria-label="Close saves"><X size={18} /></button></div>
          <div className="saves-section">
            <span><strong>Save states</strong><small>Snapshots of the whole console, restored exactly where you left off. <kbd>F2</kbd> saves slot 1, <kbd>F4</kbd> loads it.</small></span>
            <div className="state-slots">
              {STATE_SLOTS.map((slot) => {
                const record = stateSlots.get(slot)
                return (
                  <article className="state-slot" key={slot}>
                    {record?.screenshot ? <img src={record.screenshot} alt={`Slot ${slot} screenshot`} /> : <div className="state-slot-empty" aria-hidden="true">EMPTY</div>}
                    <div className="state-slot-info"><strong>Slot {slot}</strong><small>{record ? formatSavedDate(record.savedAt) : 'No state saved'}</small></div>
                    <div className="state-slot-actions">
                      <button type="button" className="secondary-button" onClick={() => void saveStateToSlot(slot)}>Save</button>
                      <button type="button" className="secondary-button" disabled={!record} onClick={() => void loadStateFromSlot(slot)}>Load</button>
                    </div>
                  </article>
                )
              })}
            </div>
          </div>
          <div className="saves-section">
            <span><strong>Save file</strong><small>{game.backupType === 'none' ? 'This cartridge has no save memory.' : `The game's own ${game.backupType} save is stored automatically whenever the game saves. Loading a state never changes it.`}</small></span>
            <div className="save-file-actions">
              <button type="button" className="secondary-button" disabled={game.emulator.backup_data().length === 0} onClick={downloadSaveFile}><Download size={16} /> Download .sav</button>
              <button type="button" className="secondary-button" disabled={game.backupType === 'none'} onClick={() => importInputRef.current?.click()}><Upload size={16} /> Import .sav</button>
              <input ref={importInputRef} className="file-input" type="file" accept=".sav,application/octet-stream" onChange={(event) => void chooseSaveFile(event)} />
            </div>
            {importError && <p className="save-import-error" role="alert">{importError}</p>}
            {pendingImport && <div className="save-import-confirm" role="alert">
              <p>Replace this game's save with <strong>{pendingImport.name}</strong>? The game restarts to read it, and unsaved progress since its last save is lost.</p>
              <div><button type="button" className="primary-button" onClick={() => void importSaveFile()}>Replace and restart</button><button type="button" className="secondary-button" onClick={() => setPendingImport(null)}>Cancel</button></div>
            </div>}
          </div>
        </section>
      </div>}
      {showSettings && <div className="settings-backdrop" role="presentation" onMouseDown={() => setShowSettings(false)}>
        <section className="settings-modal" role="dialog" aria-modal="true" aria-labelledby="settings-title" onMouseDown={(event) => event.stopPropagation()}>
          <div className="settings-modal-heading"><div><p className="eyebrow">EMULATOR</p><h2 id="settings-title">Settings</h2></div><button className="modal-close" onClick={() => setShowSettings(false)} aria-label="Close settings"><X size={18} /></button></div>
          <label className="setting-row"><span><strong>Skip startup splash</strong><small>Start directly on the game display.</small></span><input type="checkbox" checked={settings.skipStartupSplash} onChange={(event) => onSettingsChange({ ...settings, skipStartupSplash: event.target.checked })} /></label>
          <div className="setting-row setting-column">
            <span id="speed-label"><strong>Game speed</strong><small>Sound plays at the same speed, so its pitch follows.</small></span>
            <div className="speed-options" role="radiogroup" aria-labelledby="speed-label">
              {GAME_SPEED_OPTIONS.map(({ speed, label, Icon }) => (
                <button key={speed} type="button" role="radio" aria-checked={settings.speed === speed} className={settings.speed === speed ? 'speed-option active' : 'speed-option'} onClick={() => onSettingsChange({ ...settings, speed })}>
                  <Icon size={22} aria-hidden="true" /><span>{label}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="setting-row setting-column">
            <span id="sound-label"><strong>Sound</strong><small>Audio starts after your first key press or click.</small></span>
            <div className="sound-controls">
              <button type="button" className="sound-toggle" aria-pressed={!settings.sound} aria-label={settings.sound ? 'Mute' : 'Unmute'} title={settings.sound ? 'Mute' : 'Unmute'} onClick={() => onSettingsChange({ ...settings, sound: !settings.sound })}>
                {settings.sound && settings.volume > 0 ? <Volume2 size={18} /> : <VolumeX size={18} />}
              </button>
              <input type="range" min={0} max={100} step={1} aria-labelledby="sound-label" aria-valuetext={`${Math.round(settings.volume * 100)}%`} value={Math.round(settings.volume * 100)} disabled={!settings.sound} onChange={(event) => onSettingsChange({ ...settings, volume: Number(event.target.value) / 100 })} />
              <output>{settings.sound ? `${Math.round(settings.volume * 100)}%` : 'Off'}</output>
            </div>
          </div>
          <label className="setting-row"><span><strong>Touch controls</strong><small>Show on-screen GBA buttons below the game.</small></span><input type="checkbox" checked={settings.touchControls} onChange={(event) => onSettingsChange({ ...settings, touchControls: event.target.checked })} /></label>
          <div className="setting-row setting-column">
            <span id="touch-theme-label"><strong>Theme</strong><small>Colours of the mobile touch controls.</small></span>
            <div className="touch-theme-options" role="radiogroup" aria-labelledby="touch-theme-label">
              {TOUCH_THEMES.map(({ id, label }) => (
                <button key={id} type="button" role="radio" aria-checked={settings.touchTheme === id} className={settings.touchTheme === id ? 'speed-option active' : 'speed-option'} onClick={() => onSettingsChange({ ...settings, touchTheme: id })}>
                  <span className="touch-theme-swatch" data-touch-theme={id} aria-hidden="true"><i /><b /></span><span>{label}</span>
                </button>
              ))}
            </div>
          </div>
          <p className="settings-note">The GBA-WASM splash is a browser presentation only; it does not emulate the GBA BIOS boot sequence.</p>
        </section>
      </div>}
    </section>
  )
}

function SavedGamesPage({ games, onResume, onRemove }: { games: SavedGame[]; onResume: (game: SavedGame) => void; onRemove: (id: string) => void }) {
  return (
    <section className="saved-page">
      <div className="saved-heading"><div><p className="eyebrow">LOCAL LIBRARY</p><h1>Saved games</h1><p>Your ROMs and play snapshots stay on this device.</p></div><span className="library-count">{String(games.length).padStart(2, '0')}<small>GAMES</small></span></div>
      {games.length ? (
        <div className="saved-list">{games.map((game) => (
          <article className="saved-row" key={game.id}>
            <button className="saved-art" onClick={() => onResume(game)} aria-label={`Play ${game.name}`}>
            <img src={game.screenshot || `${import.meta.env.BASE_URL}logo.png`} alt={game.screenshot ? `${game.name} screenshot` : `Preview unavailable for ${game.name}`} /><span><Gamepad2 size={15} /> PLAY</span>
            </button>
            <div className="saved-info"><h2>{game.name}</h2><p>Last played {formatSavedDate(game.playedAt)}</p></div>
            <div className="saved-date"><span>LAST PLAYED</span><time dateTime={new Date(game.playedAt).toISOString()}>{formatSavedDate(game.playedAt)}</time></div>
            <button className="delete-button" onClick={() => void onRemove(game.id)} aria-label={`Remove ${game.name}`} title="Remove game"><Trash2 size={18} /></button>
          </article>
        ))}</div>
      ) : (
        <div className="empty-saved"><div className="empty-emulator-icon"><HardDrive size={27} /></div><h2>No saved games yet</h2>
          <p>Load a ROM and it will appear here with a screenshot and play date.</p><Link className="primary-button" to="/"><ArrowDownToLine size={18} /> Go to home</Link>
        </div>
      )}
    </section>
  )
}

export default App
