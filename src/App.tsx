import { useEffect, useRef, useState, type ChangeEvent } from 'react'
import { ArrowDownToLine, ArrowLeft, BookOpen, ChevronRight, CodeXml, Gamepad2, HardDrive, Settings, Trash2, X } from 'lucide-react'
import { Link, NavLink, Route, Routes, useNavigate } from 'react-router-dom'
import { loadGbaWasm, type GbaWasmEmulator } from './wasmBridge'
import { deleteSavedGame, getSavedGames, saveGame, type SavedGame } from './savedGames'

type ActiveGame = { launchId: number; name: string; status: string; pixels: Uint8ClampedArray; emulator: GbaWasmEmulator; startupLogo: string }
const DOCS_URL = '/gba-wasm/'
const SOURCE_URL = 'https://github.com/smolgroot/gba-wasm'
const GBA_BOX_ART_URL = 'https://thumbnails.libretro.com/Nintendo%20-%20Game%20Boy%20Advance/Named_Boxarts/'
const GBA_FRAME_DURATION_MS = (1000 * 280_896) / 16_777_216
const STARTUP_SPLASH_DURATION_MS = 1800
const SETTINGS_STORAGE_KEY = 'gba-wasm.settings'
const BIOS_LOGO_HUFFMAN_PREFIX = new Uint8Array([
  0x24, 0xD4, 0x00, 0x00, 0x0F, 0x40, 0x00, 0x00, 0x00, 0x01, 0x81, 0x82,
  0x82, 0x83, 0x0F, 0x83, 0x0C, 0xC3, 0x03, 0x83, 0x01, 0x83, 0x04, 0xC3,
  0x08, 0x0E, 0x02, 0xC2, 0x0D, 0xC2, 0x07, 0x0B, 0x06, 0x0A, 0x05, 0x09,
])

type EmulatorSettings = {
  skipStartupSplash: boolean
}

function readSettings(): EmulatorSettings {
  try {
    const stored = localStorage.getItem(SETTINGS_STORAGE_KEY)
    if (!stored) return { skipStartupSplash: false }
    const parsed: unknown = JSON.parse(stored)
    if (typeof parsed === 'object' && parsed !== null && 'skipStartupSplash' in parsed) {
      return { skipStartupSplash: Boolean(parsed.skipStartupSplash) }
    }
  } catch {
    // Use defaults when storage is unavailable or malformed.
  }
  return { skipStartupSplash: false }
}

function nintendoLogoImage(rom: Uint8Array) {
  const compressed = new Uint8Array(BIOS_LOGO_HUFFMAN_PREFIX.length + 156)
  compressed.set(BIOS_LOGO_HUFFMAN_PREFIX)
  compressed.set(rom.subarray(0x04, 0xA0), BIOS_LOGO_HUFFMAN_PREFIX.length)

  const decoded = new Uint8Array(212)
  const treeRoot = 5
  let node = treeRoot
  let outputNibble = 0
  let outputByte = 0
  for (let wordOffset = 36; wordOffset + 3 < compressed.length && outputByte < decoded.length; wordOffset += 4) {
    for (let byteOffset = 3; byteOffset >= 0 && outputByte < decoded.length; byteOffset--) {
      for (let bit = 7; bit >= 0 && outputByte < decoded.length; bit--) {
        const direction = (compressed[wordOffset + byteOffset] >>> bit) & 1
        const nodeValue = compressed[node]
        const child = (node & ~1) + (((nodeValue & 0x3F) + 1) << 1) + direction
        if (((nodeValue << direction) & 0x80) === 0) {
          node = child
          continue
        }
        const symbol = compressed[child] & 0x0F
        if (outputNibble === 0) {
          decoded[outputByte] = symbol
          outputNibble = 1
        } else {
          decoded[outputByte++] |= symbol << 4
          outputNibble = 0
        }
        node = treeRoot
      }
    }
  }

  const pixels = new Uint8ClampedArray(104 * 16 * 4)
  let previous = 0
  for (let offset = 0; offset < 208; offset += 2) {
    previous = (previous + decoded[4 + offset] + (decoded[5 + offset] << 8)) & 0xFFFF
    const tileByte = previous & 0xFF
    const tileByteHigh = previous >>> 8
    decoded[4 + offset] = tileByte
    decoded[5 + offset] = tileByteHigh
  }
  for (let y = 0; y < 16; y++) {
    for (let x = 0; x < 104; x++) {
      const tile = Math.floor(x / 8) + Math.floor(y / 8) * 13
      const byte = decoded[4 + tile * 8 + (y % 8)]
      if (((byte >>> (x % 8)) & 1) === 0) continue
      const index = (y * 104 + x) * 4
      pixels[index] = 33
      pixels[index + 1] = 60
      pixels[index + 2] = 166
      pixels[index + 3] = 255
    }
  }
  const canvas = document.createElement('canvas')
  canvas.width = 104
  canvas.height = 16
  canvas.getContext('2d')?.putImageData(new ImageData(pixels, 104, 16), 0, 0)
  return canvas.toDataURL('image/png')
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
  return <img className={className} src={showFallback ? fallback : gbaBoxArtUrl(name)} alt={alt} onError={() => setShowFallback(true)} />
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
    return () => {
      mounted = false
      emulatorRef.current?.free()
      emulatorRef.current = null
    }
  }, [])

  async function executeRom(rom: Uint8Array, name: string) {
    const startupLogo = nintendoLogoImage(rom)
    const wasm = await loadGbaWasm()
    const info = wasm.inspect_rom(rom)
    const emulator = wasm.create_emulator(rom)
    let pixels: Uint8ClampedArray
    try {
      pixels = emulator.step_frame()
    } catch (error) {
      emulator.free()
      throw error
    }
    emulatorRef.current?.free()
    emulatorRef.current = emulator
    const status = `${info}: running`
    setActiveGame({ launchId: ++launchIdRef.current, name, status, pixels, emulator, startupLogo })
    setAppStatus(status)
    return pixels
  }

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
        screenshot: fileToImage(pixels),
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
          <Route path="/play" element={<EmulatorPage key={activeGame?.launchId ?? 'empty'} game={activeGame} status={appStatus} errorLog={romLoadLog} settings={settings} onSettingsChange={updateSettings} onLoad={triggerLoad} onRuntimeError={setAppStatus} />} />
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

function EmulatorPage({ game, status, errorLog, settings, onSettingsChange, onLoad, onRuntimeError }: { game: ActiveGame | null; status: string; errorLog: string | null; settings: EmulatorSettings; onSettingsChange: (settings: EmulatorSettings) => void; onLoad: () => void; onRuntimeError: (status: string) => void }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const [showSettings, setShowSettings] = useState(false)
  const [showStartupSplash, setShowStartupSplash] = useState(() => !settings.skipStartupSplash)
  const splashVisible = showStartupSplash && !settings.skipStartupSplash

  useEffect(() => {
    if (!splashVisible) return
    const timer = window.setTimeout(() => setShowStartupSplash(false), STARTUP_SPLASH_DURATION_MS)
    return () => window.clearTimeout(timer)
  }, [splashVisible])

  useEffect(() => {
    const context = canvasRef.current?.getContext('2d')
    if (!game || !context) return

    context.putImageData(new ImageData(new Uint8ClampedArray(game.pixels), 240, 160), 0, 0)
    let animationFrame = 0
    let running = true
    let nextFrameAt: number | null = null
    const advanceFrame = (timestamp: number) => {
      if (!running) return
      if (nextFrameAt === null || timestamp >= nextFrameAt) {
        try {
          const pixels = game.emulator.step_frame()
          context.putImageData(new ImageData(new Uint8ClampedArray(pixels), 240, 160), 0, 0)
          nextFrameAt = timestamp + GBA_FRAME_DURATION_MS
        } catch (error) {
          running = false
          onRuntimeError(error instanceof Error ? error.message : String(error))
          return
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

  return (
    <section className="emulator-page">
      <div className="emulator-heading"><div><p className="eyebrow">NOW PLAYING</p><h1>{game.name}</h1></div><div className="emulator-heading-actions"><button className="secondary-button" onClick={() => setShowSettings(true)}><Settings size={17} /> Settings</button><Link to="/saved-games" className="secondary-button"><HardDrive size={17} /> Library</Link></div></div>
      <div className="emulator-layout">
        <div className="play-surface">
          <div className="play-surface-top"><span><i /> EMULATOR ONLINE</span><span>240 × 160</span></div>
          <div className="screen-well"><canvas ref={canvasRef} width={240} height={160} />
            {splashVisible && <button className="startup-splash" onClick={() => setShowStartupSplash(false)} aria-label="Skip startup splash"><img src={game.startupLogo} alt="Nintendo" /><small>Tap to skip</small></button>}
          </div>
          <div className="play-surface-bottom"><span>{status}</span><span>GBA / 32-BIT</span></div>
        </div>
        <aside className="emulator-aside">
          <CartridgeMockup key={game.name} name={game.name} />
          <p>ROM loaded into the WebAssembly runtime.</p>
          <button className="secondary-button wide-button" onClick={onLoad}><ArrowDownToLine size={17} /> Replace ROM</button>
          <div className="keyboard-note"><span>CONTROLS</span><div><kbd>Z</kbd><kbd>X</kbd><kbd>ENTER</kbd><kbd>SHIFT</kbd></div></div>
        </aside>
      </div>
      <p className="emulator-disclaimer">The emulator core is in early development. CPU, BIOS, timing, and GPU support remain incomplete, so most retail ROMs will not boot yet.</p>
      {showSettings && <div className="settings-backdrop" role="presentation" onMouseDown={() => setShowSettings(false)}>
        <section className="settings-modal" role="dialog" aria-modal="true" aria-labelledby="settings-title" onMouseDown={(event) => event.stopPropagation()}>
          <div className="settings-modal-heading"><div><p className="eyebrow">EMULATOR</p><h2 id="settings-title">Settings</h2></div><button className="modal-close" onClick={() => setShowSettings(false)} aria-label="Close settings"><X size={18} /></button></div>
          <label className="setting-row"><span><strong>Skip startup splash</strong><small>Start directly on the game display.</small></span><input type="checkbox" checked={settings.skipStartupSplash} onChange={(event) => onSettingsChange({ ...settings, skipStartupSplash: event.target.checked })} /></label>
          <p className="settings-note">The splash is a browser presentation only; it does not emulate the GBA BIOS boot sequence.</p>
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
              <img src={game.screenshot} alt={`${game.name} screenshot`} /><span><Gamepad2 size={15} /> PLAY</span>
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
