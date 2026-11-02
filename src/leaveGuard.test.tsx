import { act, StrictMode } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useLeaveGuard } from './leaveGuard'

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true

function Guarded({ active, onBack }: { active: boolean; onBack: () => void }) {
  useLeaveGuard(active, onBack)
  return null
}

/** jsdom dispatches `popstate` from `history.back()` asynchronously. */
async function goBack() {
  await act(async () => {
    const popped = new Promise((resolve) => window.addEventListener('popstate', resolve, { once: true }))
    window.history.back()
    await popped
  })
}

const isGuardEntry = () => (window.history.state as { usr?: { leaveGuard?: boolean } } | null)?.usr?.leaveGuard === true

describe('useLeaveGuard', () => {
  let container: HTMLDivElement
  let root: Root
  let onBack: ReturnType<typeof vi.fn<() => void>>

  beforeEach(() => {
    window.history.replaceState(null, '', '/play')
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
    onBack = vi.fn<() => void>()
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
  })

  function render(active: boolean) {
    act(() => root.render(<StrictMode><BrowserRouter><Guarded active={active} onBack={onBack} /></BrowserRouter></StrictMode>))
  }

  it('pushes a single guard entry, even under StrictMode', () => {
    const before = window.history.length
    render(true)
    expect(window.history.length).toBe(before + 1)
    expect(isGuardEntry()).toBe(true)
    expect(window.location.pathname).toBe('/play')
  })

  it('asks instead of leaving on back, and re-arms', async () => {
    render(true)
    await goBack()
    expect(onBack).toHaveBeenCalledTimes(1)
    expect(isGuardEntry()).toBe(true)
    expect(window.location.pathname).toBe('/play')

    await goBack()
    expect(onBack).toHaveBeenCalledTimes(2)
    expect(isGuardEntry()).toBe(true)
  })

  it('does nothing while inactive', () => {
    const before = window.history.length
    render(false)
    expect(window.history.length).toBe(before)
    expect(isGuardEntry()).toBe(false)
  })
})
