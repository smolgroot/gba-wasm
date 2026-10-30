import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Chip from '@mui/material/Chip'
import IconButton from '@mui/material/IconButton'
import Slider from '@mui/material/Slider'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import useMediaQuery from '@mui/material/useMediaQuery'
import ChevronLeft from '@mui/icons-material/ChevronLeft'
import ChevronRight from '@mui/icons-material/ChevronRight'
import PlayArrowRounded from '@mui/icons-material/PlayArrowRounded'
import FileUploadOutlined from '@mui/icons-material/FileUploadOutlined'
import { gbaBoxArtUrl, type ShowcaseItem } from './featuredGames'
import { VelocityTracker, clamp, inertiaTarget, isAtRest, stepSpring, withOverscroll } from './coverFlowMotion'

const AUTOPLAY_INTERVAL_MS = 3500

/**
 * Cover images to try in order. Featured cartridges use the thumbnail written by `pnpm run boxart:fetch` and
 * precached by the service worker; other library games try the online box art, then their saved screenshot.
 */
function coverSources(item: ShowcaseItem) {
  if (item.featured) return [`${import.meta.env.BASE_URL}boxart/${item.featured.id}.webp`]
  return [item.saved && gbaBoxArtUrl(item.saved.name), item.saved?.screenshot].filter((source): source is string => Boolean(source))
}

function formatPlayedDate(timestamp: number) {
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' }).format(timestamp)
}

// Horizontal drag, in pixels, before a press on the covers becomes a drag instead of a click.
const DRAG_THRESHOLD_PX = 6
// Wheel and trackpad scrolling settle on the nearest cover once they pause this long.
const WHEEL_SETTLE_MS = 140
// Covers further than this from the centre fade out.
const VISIBLE_SIDE_COVERS = 4

type CoverFlowProps = {
  items: ShowcaseItem[]
  onLaunch: (item: ShowcaseItem) => void
}

/**
 * The scroll position in covers, animated by a spring towards a target cover. Drags and scrolls set it
 * directly; releasing hands the current velocity to the spring, which gives the scroll its inertia.
 */
function useScrollPosition(reducedMotion: boolean) {
  const [position, setPosition] = useState(0)
  const motionRef = useRef({ position: 0, velocity: 0, target: 0, frame: 0 })

  /** Moves straight to `next`, stopping any animation. */
  const jumpTo = useCallback((next: number) => {
    const motion = motionRef.current
    cancelAnimationFrame(motion.frame)
    Object.assign(motion, { position: next, velocity: 0, target: next, frame: 0 })
    setPosition(next)
  }, [])

  /** Springs to the cover `target`, starting at `velocity` (covers per second) or the current one if omitted. */
  const settleOn = useCallback((target: number, velocity?: number) => {
    const motion = motionRef.current
    if (reducedMotion) return jumpTo(target)
    motion.target = target
    if (velocity !== undefined) motion.velocity = velocity
    if (motion.frame) return
    let previous = performance.now()
    const tick = (now: number) => {
      // Capped so a frame after a background tab does not jump the covers.
      const seconds = Math.min((now - previous) / 1000, 1 / 30)
      previous = now
      Object.assign(motion, stepSpring(motion, motion.target, seconds))
      if (isAtRest(motion, motion.target)) Object.assign(motion, { position: motion.target, velocity: 0, frame: 0 })
      else motion.frame = requestAnimationFrame(tick)
      setPosition(motion.position)
    }
    motion.frame = requestAnimationFrame(tick)
  }, [reducedMotion, jumpTo])

  useEffect(() => {
    const motion = motionRef.current
    return () => cancelAnimationFrame(motion.frame)
  }, [])

  return { position, motionRef, jumpTo, settleOn }
}

/**
 * Cover-flow slideshow: the centred cover faces the viewer, the others fan out in perspective on each side.
 * It scrolls continuously by dragging, swiping or horizontal wheel scrolling, then glides to rest on a cover.
 * Games not in the library are shown in greyscale.
 */
export function CoverFlow({ items, onLaunch }: CoverFlowProps) {
  const [paused, setPaused] = useState(false)
  // Number of cover sources that failed to load, per item.
  const [failedArt, setFailedArt] = useState<Readonly<Record<string, number>>>({})
  const [velocityTracker] = useState(() => new VelocityTracker())
  const listRef = useRef<HTMLDivElement | null>(null)
  const dragRef = useRef<{ startX: number; startPosition: number; dragging: boolean } | null>(null)
  const compact = useMediaQuery('(max-width: 620px)')
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)')
  const { position, motionRef, jumpTo, settleOn } = useScrollPosition(reducedMotion)
  const coverSize = compact ? 150 : 220
  // Distance from the centre to the first side cover; also the drag distance that scrolls by one cover.
  const sideGap = coverSize * 0.72
  // Spacing between the stacked side covers.
  const stackGap = coverSize * 0.3
  const last = items.length - 1
  // Covers to the edge of the list stay in reach even when the library shrinks under the current position.
  const shown = clamp(position, -1, last + 1)
  const active = clamp(Math.round(shown), 0, last)
  const current = items[active]
  const inLibrary = Boolean(current?.saved)

  // Advances on its own until the player hovers, focuses or touches the carousel.
  useEffect(() => {
    if (paused || reducedMotion || items.length < 2) return
    const timer = window.setInterval(() => settleOn((Math.round(motionRef.current.target) + 1) % items.length), AUTOPLAY_INTERVAL_MS)
    return () => window.clearInterval(timer)
  }, [paused, reducedMotion, items.length, settleOn, motionRef])

  // Horizontal wheel and trackpad scrolling; the listener is not passive so it can keep the browser from
  // treating the gesture as back/forward navigation. Vertical scrolling is left to the page.
  useEffect(() => {
    const list = listRef.current
    if (!list) return
    let settleTimer = 0
    const handleWheel = (event: WheelEvent) => {
      const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.shiftKey ? event.deltaY : 0
      if (!delta) return
      event.preventDefault()
      setPaused(true)
      const pixels = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? delta * 16 : delta
      const from = motionRef.current.position
      jumpTo(clamp(from + (pixels / sideGap) * (from < 0 || from > last ? 0.3 : 1), -0.5, last + 0.5))
      window.clearTimeout(settleTimer)
      settleTimer = window.setTimeout(() => settleOn(clamp(Math.round(motionRef.current.position), 0, last)), WHEEL_SETTLE_MS)
    }
    list.addEventListener('wheel', handleWheel, { passive: false })
    return () => {
      list.removeEventListener('wheel', handleWheel)
      window.clearTimeout(settleTimer)
    }
  }, [jumpTo, settleOn, motionRef, sideGap, last])

  if (!current) return null

  /** Glides one cover left or right of where the carousel is heading. */
  function step(direction: -1 | 1) {
    setPaused(true)
    settleOn(clamp(Math.round(motionRef.current.target) + direction, 0, last))
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === 'ArrowLeft') step(-1)
    else if (event.key === 'ArrowRight') step(1)
    else if (event.key === 'Enter' && event.target === event.currentTarget) onLaunch(current)
    else return
    event.preventDefault()
  }

  function handlePointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return
    setPaused(true)
    // Grabbing the covers stops them where they are.
    jumpTo(motionRef.current.position)
    dragRef.current = { startX: event.clientX, startPosition: motionRef.current.position, dragging: false }
    velocityTracker.reset()
    velocityTracker.add(event.timeStamp, motionRef.current.position)
  }

  function handlePointerMove(event: PointerEvent<HTMLDivElement>) {
    const drag = dragRef.current
    if (!drag) return
    const distance = event.clientX - drag.startX
    if (!drag.dragging) {
      if (Math.abs(distance) < DRAG_THRESHOLD_PX) return
      drag.dragging = true
      // Captured only once dragging, so a plain press still clicks the cover under it.
      event.currentTarget.setPointerCapture(event.pointerId)
    }
    const next = withOverscroll(drag.startPosition - distance / sideGap, last)
    jumpTo(next)
    velocityTracker.add(event.timeStamp, next)
  }

  function handlePointerUp(event: PointerEvent<HTMLDivElement>) {
    const drag = dragRef.current
    dragRef.current = null
    if (!drag) return
    // A press that never became a drag only stopped the covers; let them settle. A click on a cover retargets this.
    if (!drag.dragging) return settleOn(clamp(Math.round(motionRef.current.position), 0, last))
    const velocity = velocityTracker.velocity(event.timeStamp)
    settleOn(inertiaTarget(motionRef.current.position, velocity, last), velocity)
  }

  function handlePointerCancel() {
    const drag = dragRef.current
    dragRef.current = null
    if (drag?.dragging) settleOn(clamp(Math.round(motionRef.current.position), 0, last))
  }

  function handleCoverClick(index: number) {
    if (index === active) onLaunch(current)
    else {
      setPaused(true)
      settleOn(index)
    }
  }

  return (
    <Box
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      sx={{ position: 'relative', userSelect: 'none' }}
    >
      <Box
        ref={listRef}
        role="listbox"
        tabIndex={0}
        aria-label="Cartridges"
        aria-activedescendant={`cover-${current.id}`}
        onKeyDown={handleKeyDown}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerCancel}
        sx={{
          position: 'relative',
          height: coverSize * 1.55,
          perspective: compact ? 600 : 900,
          overflow: 'hidden',
          touchAction: 'pan-y',
          cursor: 'grab',
          outline: 'none',
          '&:active': { cursor: 'grabbing' },
          '&:focus-visible': { boxShadow: (theme) => `inset 0 0 0 1px ${theme.palette.primary.main}` },
        }}
      >
        {items.map((item, index) => {
          // Fractional while scrolling: covers turn and slide continuously between their resting places.
          const offset = index - shown
          const distance = Math.abs(offset)
          const side = Math.sign(offset)
          const turn = Math.min(distance, 1)
          const shift = side * (turn * sideGap + Math.max(0, distance - 1) * stackGap)
          const cover = coverSources(item)[failedArt[item.id] ?? 0]
          const markArtFailed = () => setFailedArt((failed) => ({ ...failed, [item.id]: (failed[item.id] ?? 0) + 1 }))
          // Side covers darken as they recede, like the original cover flow; games not in the library lose their colour.
          const brightness = distance < 1 ? 1 - 0.32 * distance : Math.max(0.35, 0.8 - distance * 0.12)
          const visibility = clamp(VISIBLE_SIDE_COVERS + 1 - distance, 0, 1)
          return (
            <Box
              key={item.id}
              id={`cover-${item.id}`}
              role="option"
              aria-selected={index === active}
              aria-label={item.saved ? `${item.title}, in your library` : item.title}
              onClick={() => handleCoverClick(index)}
              // Per-frame values go through `style`, so scrolling does not generate a new class every frame.
              style={{
                zIndex: Math.round(1000 - distance * 10),
                transform: `translateX(calc(-50% + ${shift}px)) translateZ(${60 - 140 * turn}px) rotateY(${-side * 62 * turn}deg)`,
                opacity: visibility,
                pointerEvents: visibility > 0 ? 'auto' : 'none',
                filter: `${item.saved ? '' : 'grayscale(1) '}brightness(${brightness})`,
              }}
              sx={{
                position: 'absolute',
                top: compact ? 12 : 20,
                left: '50%',
                width: coverSize,
                height: coverSize,
                transformStyle: 'preserve-3d',
                cursor: 'pointer',
              }}
            >
              {!cover ? (
                <Box sx={{ width: '100%', height: '100%', display: 'grid', placeContent: 'center', gap: 1, p: 2, textAlign: 'center', bgcolor: 'background.paper', border: 1, borderColor: 'divider' }}>
                  <Typography sx={{ font: '9px/1.8 var(--pixel)', color: 'primary.light' }}>{item.title}</Typography>
                  <Typography sx={{ font: '9px var(--mono)', color: 'text.secondary' }}>GAME BOY ADVANCE</Typography>
                </Box>
              ) : (
                <>
                  <Box
                    component="img"
                    src={cover}
                    alt=""
                    draggable={false}
                    loading="lazy"
                    onError={markArtFailed}
                    sx={{ width: '100%', height: '100%', display: 'block', objectFit: 'cover', boxShadow: '0 18px 40px #000a' }}
                  />
                  {/* Mirrored copy fading out below the cover, the floor reflection of the iPod cover flow. */}
                  <Box
                    component="img"
                    src={cover}
                    alt=""
                    aria-hidden
                    draggable={false}
                    loading="lazy"
                    sx={{
                      position: 'absolute',
                      top: '100%',
                      left: 0,
                      width: '100%',
                      height: '100%',
                      mt: '2px',
                      objectFit: 'cover',
                      transform: 'scaleY(-1)',
                      opacity: 0.32,
                      maskImage: 'linear-gradient(to top, #000 0%, transparent 45%)',
                      WebkitMaskImage: 'linear-gradient(to top, #000 0%, transparent 45%)',
                      pointerEvents: 'none',
                    }}
                  />
                </>
              )}
            </Box>
          )
        })}
      </Box>

      <Stack spacing={1.5} sx={{ alignItems: 'center', mt: compact ? 0 : -1, px: 2 }}>
        <Typography component="h3" sx={{ m: 0, color: '#f2f2ec', font: compact ? '10px/1.8 var(--pixel)' : '12px/1.8 var(--pixel)', textAlign: 'center' }}>
          {current.title}
        </Typography>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
          <Typography sx={{ color: 'text.secondary', font: '11px var(--mono)' }}>
            {current.saved ? `Played ${formatPlayedDate(current.saved.playedAt)}` : current.featured?.year}
          </Typography>
          <Chip
            size="small"
            variant="outlined"
            color={inLibrary ? 'primary' : 'default'}
            label={inLibrary ? 'In your library' : 'Bring your own ROM'}
            sx={{ font: '10px var(--mono)', borderRadius: 1 }}
          />
        </Stack>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
          <IconButton aria-label="Previous cartridge" onClick={() => step(-1)} disabled={active === 0}>
            <ChevronLeft />
          </IconButton>
          <Button
            variant="contained"
            disableElevation
            startIcon={inLibrary ? <PlayArrowRounded /> : <FileUploadOutlined />}
            onClick={() => onLaunch(current)}
            sx={{ minHeight: 44, px: 2.5 }}
          >
            {inLibrary ? 'Play now' : 'Insert cartridge'}
          </Button>
          <IconButton aria-label="Next cartridge" onClick={() => step(1)} disabled={active === last}>
            <ChevronRight />
          </IconButton>
        </Stack>
        {/* The scrubber under the covers, as on the iPod. */}
        <Slider
          aria-label="Browse cartridges"
          size="small"
          min={0}
          max={last}
          step={1}
          // Follows the scroll position, so the thumb glides along with the covers.
          value={clamp(shown, 0, last)}
          onChange={(_, value) => { setPaused(true); settleOn(value as number) }}
          getAriaValueText={(value) => items[value]?.title ?? ''}
          sx={{ width: compact ? '70%' : 320, color: 'primary.main', '& .MuiSlider-thumb': { width: 10, height: 10, borderRadius: 0 } }}
        />
      </Stack>
    </Box>
  )
}
