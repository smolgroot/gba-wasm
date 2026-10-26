import { useEffect, useRef, useState, type ChangeEvent } from 'react'
import { ArrowDownToLine, ArrowLeft, BookOpen, ChevronRight, CodeXml, Gamepad2, HardDrive, Trash2 } from 'lucide-react'
import { Link, NavLink, Route, Routes, useNavigate } from 'react-router-dom'
import { loadGbaWasm } from './wasmBridge'
import { deleteSavedGame, getSavedGames, saveGame, type SavedGame } from './savedGames'

type ActiveGame = { name: string; status: string; pixels: Uint8ClampedArray }
const DOCS_URL = '/gba-wasm/'
const SOURCE_URL = 'https://github.com/smolgroot/gba-wasm'

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

function App() {
  const inputRef = useRef<HTMLInputElement | null>(null)
  const navigate = useNavigate()
  const [activeGame, setActiveGame] = useState<ActiveGame | null>(null)
  const [savedGames, setSavedGames] = useState<SavedGame[]>([])
  const [appStatus, setAppStatus] = useState('Initializing WebAssembly')
  const [romLoadLog, setRomLoadLog] = useState<string | null>(null)

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
    return () => { mounted = false }
  }, [])

  async function executeRom(rom: Uint8Array, name: string) {
    const wasm = await loadGbaWasm()
    const info = wasm.inspect_rom(rom)
    const pixels = wasm.run_rom(rom, 1000)
    const status = `${info}: executed 1000 instructions`
    setActiveGame({ name, status, pixels })
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
          <a className="nav-icon-link" href={SOURCE_URL} target="_blank" rel="noreferrer" aria-label="Source code" title="Source code"><CodeXml size={18} /></a>
          <button className="header-load" onClick={triggerLoad}><ArrowDownToLine size={17} /><span>Load ROM</span></button>
        </nav>
        <input ref={inputRef} className="file-input" type="file" accept=".gba,.bin,application/octet-stream" onChange={(event) => void loadRomFile(event)} />
      </header>
      <main>
        <Routes>
          <Route path="/" element={<HomePage savedGames={savedGames} onLoad={triggerLoad} onResume={resumeGame} />} />
          <Route path="/play" element={<EmulatorPage game={activeGame} status={appStatus} errorLog={romLoadLog} onLoad={triggerLoad} />} />
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
              <img src={game.screenshot} alt={`${game.name} screenshot`} />
              <span><strong>{game.name}</strong><small>{formatSavedDate(game.playedAt)}</small></span><ChevronRight size={17} />
            </button>
          ))}</div>
        ) : <div className="empty-library"><Gamepad2 size={20} /><span>Your library is ready for its first game.</span></div>}
      </section>
    </div>
  )
}

function EmulatorPage({ game, status, errorLog, onLoad }: { game: ActiveGame | null; status: string; errorLog: string | null; onLoad: () => void }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  useEffect(() => {
    const context = canvasRef.current?.getContext('2d')
    if (game && context) context.putImageData(new ImageData(new Uint8ClampedArray(game.pixels), 240, 160), 0, 0)
  }, [game])

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
      <div className="emulator-heading"><div><p className="eyebrow">NOW PLAYING</p><h1>{game.name}</h1></div><Link to="/saved-games" className="secondary-button"><HardDrive size={17} /> Library</Link></div>
      <div className="emulator-layout">
        <div className="play-surface">
          <div className="play-surface-top"><span><i /> EMULATOR ONLINE</span><span>240 × 160</span></div>
          <div className="screen-well"><canvas ref={canvasRef} width={240} height={160} /></div>
          <div className="play-surface-bottom"><span>{status}</span><span>GBA / 32-BIT</span></div>
        </div>
        <aside className="emulator-aside"><p className="eyebrow">CARTRIDGE</p><h2>{game.name}</h2><p>ROM loaded into the WebAssembly runtime.</p>
          <button className="secondary-button wide-button" onClick={onLoad}><ArrowDownToLine size={17} /> Replace ROM</button>
          <div className="keyboard-note"><span>CONTROLS</span><div><kbd>Z</kbd><kbd>X</kbd><kbd>ENTER</kbd><kbd>SHIFT</kbd></div></div>
        </aside>
      </div>
      <p className="emulator-disclaimer">The emulator core is in early development. CPU, BIOS, timing, and GPU support remain incomplete, so most retail ROMs will not boot yet.</p>
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
