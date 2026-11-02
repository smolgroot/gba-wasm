import { useEffect, useRef } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'

type GuardState = { leaveGuard: true }

// React Router keeps navigation state under `usr`; reading it from `history.state` sees a push
// immediately, so StrictMode's repeated effect does not push a second entry.
function isGuardEntry() {
  const state: unknown = window.history.state
  return typeof state === 'object' && state !== null && (state as { usr?: Partial<GuardState> }).usr?.leaveGuard === true
}

/**
 * While `active`, keeps an extra history entry on top of the current page, so the browser back
 * button and the mobile back gesture land on a copy of this page instead of leaving it. Each
 * such back calls `onBackRequest` and puts the entry back; the caller decides whether to leave.
 */
export function useLeaveGuard(active: boolean, onBackRequest: () => void) {
  const navigate = useNavigate()
  const { pathname, key } = useLocation()
  const onBackRequestRef = useRef(onBackRequest)

  useEffect(() => {
    onBackRequestRef.current = onBackRequest
  }, [onBackRequest])

  // Re-arms after every navigation, including pushes to this page that carry no guard.
  useEffect(() => {
    if (!active || isGuardEntry()) return
    navigate(pathname, { state: { leaveGuard: true } satisfies GuardState })
  }, [active, pathname, key, navigate])

  useEffect(() => {
    if (!active) return
    const onPopState = () => {
      if (isGuardEntry()) return
      navigate(pathname, { state: { leaveGuard: true } satisfies GuardState })
      onBackRequestRef.current()
    }
    window.addEventListener('popstate', onPopState)
    return () => window.removeEventListener('popstate', onPopState)
  }, [active, pathname, navigate])
}
