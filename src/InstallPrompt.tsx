import { useEffect, useState } from 'react'
import { Download, Share, X } from 'lucide-react'

const DISMISS_STORAGE_KEY = 'gba-wasm-install-dismissed-at'
// A dismissed prompt stays hidden for this long before it may be offered again.
const DISMISS_DURATION_MS = 14 * 24 * 60 * 60 * 1000

/** Chromium's install event; it is not part of the TypeScript DOM library. */
interface BeforeInstallPromptEvent extends Event {
  prompt: () => Promise<void>
  userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }>
}

type InstallMode = { kind: 'native'; event: BeforeInstallPromptEvent } | { kind: 'ios' }

function isInstalled() {
  const standalone = (navigator as Navigator & { standalone?: boolean }).standalone === true
  return standalone || ['standalone', 'fullscreen', 'minimal-ui'].some((mode) => window.matchMedia(`(display-mode: ${mode})`).matches)
}

/** iOS and iPadOS browsers can only install through Safari's Share > Add to Home Screen. */
function isIosSafari() {
  const ua = navigator.userAgent
  const ios = /iPad|iPhone|iPod/.test(ua) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
  return ios && /Safari/.test(ua) && !/CriOS|FxiOS|EdgiOS|OPiOS/.test(ua)
}

function wasRecentlyDismissed() {
  try {
    const dismissedAt = Number(localStorage.getItem(DISMISS_STORAGE_KEY))
    return Number.isFinite(dismissedAt) && Date.now() - dismissedAt < DISMISS_DURATION_MS
  } catch {
    return false
  }
}

function rememberDismissal() {
  try {
    localStorage.setItem(DISMISS_STORAGE_KEY, String(Date.now()))
  } catch {
    // The prompt stays hidden for this session if storage is unavailable.
  }
}

/** Offers to install the PWA on browsers that support it, unless it is installed or was dismissed. */
export function InstallPrompt({ hidden }: { hidden: boolean }) {
  const [mode, setMode] = useState<InstallMode | null>(() => (!isInstalled() && !wasRecentlyDismissed() && isIosSafari() ? { kind: 'ios' } : null))

  useEffect(() => {
    if (isInstalled()) return
    const onBeforeInstall = (event: Event) => {
      // Keep the browser's mini-infobar from showing so our own prompt can trigger install.
      event.preventDefault()
      if (!wasRecentlyDismissed()) setMode({ kind: 'native', event: event as BeforeInstallPromptEvent })
    }
    const onInstalled = () => setMode(null)
    window.addEventListener('beforeinstallprompt', onBeforeInstall)
    window.addEventListener('appinstalled', onInstalled)
    return () => {
      window.removeEventListener('beforeinstallprompt', onBeforeInstall)
      window.removeEventListener('appinstalled', onInstalled)
    }
  }, [])

  if (!mode || hidden) return null

  const dismiss = () => {
    rememberDismissal()
    setMode(null)
  }

  const install = async () => {
    if (mode.kind !== 'native') return
    await mode.event.prompt()
    const { outcome } = await mode.event.userChoice
    // The event can only be prompted once; a declined install counts as a dismissal.
    if (outcome === 'dismissed') rememberDismissal()
    setMode(null)
  }

  return (
    <aside className="install-prompt" role="dialog" aria-labelledby="install-prompt-title">
      <img src={`${import.meta.env.BASE_URL}icons/icon-192.png`} alt="" />
      <div className="install-prompt-copy">
        <strong id="install-prompt-title">Install GBA-WASM</strong>
        {mode.kind === 'native'
          ? <p>Play fullscreen and offline, straight from your home screen.</p>
          : <p>Tap <Share size={13} aria-label="Share" /> then <b>Add to Home Screen</b> to play fullscreen and offline.</p>}
      </div>
      {mode.kind === 'native' && <button className="primary-button" onClick={() => void install()}><Download size={16} /> Install</button>}
      <button className="install-prompt-close" onClick={dismiss} aria-label="Dismiss install prompt" title="Not now"><X size={18} /></button>
    </aside>
  )
}
