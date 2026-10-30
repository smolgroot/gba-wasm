import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'
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
import type { FeaturedGame } from './featuredGames'

const AUTOPLAY_INTERVAL_MS = 3500

/** Local thumbnail written by `pnpm run boxart:fetch` and precached by the service worker. */
function coverUrl(game: FeaturedGame) {
  return `${import.meta.env.BASE_URL}boxart/${game.id}.webp`
}
// Horizontal drag, in pixels, that counts as a swipe to the next cover.
const SWIPE_THRESHOLD_PX = 40
// Covers further than this from the centre are hidden.
const VISIBLE_SIDE_COVERS = 4

type CoverFlowProps = {
  games: FeaturedGame[]
  isInLibrary: (game: FeaturedGame) => boolean
  onLaunch: (game: FeaturedGame) => void
}

/** Cover-flow slideshow: the centred cover faces the viewer, the others fan out in perspective on each side. */
export function CoverFlow({ games, isInLibrary, onLaunch }: CoverFlowProps) {
  const [active, setActive] = useState(0)
  const [paused, setPaused] = useState(false)
  const [brokenArt, setBrokenArt] = useState<ReadonlySet<string>>(new Set())
  const dragStartRef = useRef<number | null>(null)
  // Set by a swipe so the click that ends it does not also select or launch a cover.
  const swipedRef = useRef(false)
  const compact = useMediaQuery('(max-width: 620px)')
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)')
  const coverSize = compact ? 150 : 220
  const current = games[active]
  const inLibrary = current ? isInLibrary(current) : false

  // Advances on its own until the player hovers, focuses or touches the carousel.
  useEffect(() => {
    if (paused || reducedMotion || games.length < 2) return
    const timer = window.setInterval(() => setActive((index) => (index + 1) % games.length), AUTOPLAY_INTERVAL_MS)
    return () => window.clearInterval(timer)
  }, [paused, reducedMotion, games.length])

  if (!current) return null

  const select = (index: number) => setActive(Math.max(0, Math.min(games.length - 1, index)))

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === 'ArrowLeft') select(active - 1)
    else if (event.key === 'ArrowRight') select(active + 1)
    else if (event.key === 'Enter' && event.target === event.currentTarget) onLaunch(current)
    else return
    event.preventDefault()
    setPaused(true)
  }

  function handlePointerDown(event: PointerEvent<HTMLDivElement>) {
    dragStartRef.current = event.clientX
    swipedRef.current = false
    setPaused(true)
  }

  function handlePointerUp(event: PointerEvent<HTMLDivElement>) {
    if (dragStartRef.current === null) return
    const distance = event.clientX - dragStartRef.current
    dragStartRef.current = null
    swipedRef.current = Math.abs(distance) >= SWIPE_THRESHOLD_PX
    if (swipedRef.current) select(active + (distance < 0 ? 1 : -1))
  }

  function handleCoverClick(index: number) {
    if (swipedRef.current) swipedRef.current = false
    else if (index === active) onLaunch(current)
    else select(index)
  }

  return (
    <Box
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      sx={{ position: 'relative', userSelect: 'none' }}
    >
      <Box
        role="listbox"
        tabIndex={0}
        aria-label="Featured cartridges"
        aria-activedescendant={`cover-${current.id}`}
        onKeyDown={handleKeyDown}
        onPointerDown={handlePointerDown}
        onPointerUp={handlePointerUp}
        onPointerCancel={() => { dragStartRef.current = null }}
        sx={{
          position: 'relative',
          height: coverSize * 1.55,
          perspective: compact ? 600 : 900,
          overflow: 'hidden',
          touchAction: 'pan-y',
          outline: 'none',
          '&:focus-visible': { boxShadow: (theme) => `inset 0 0 0 1px ${theme.palette.primary.main}` },
        }}
      >
        {games.map((game, index) => {
          const offset = index - active
          const distance = Math.abs(offset)
          const side = Math.sign(offset)
          const shift = distance === 0 ? 0 : side * (coverSize * 0.72 + (distance - 1) * coverSize * 0.3)
          const transform = distance === 0
            ? 'translateX(-50%) translateZ(60px)'
            : `translateX(calc(-50% + ${shift}px)) translateZ(-80px) rotateY(${-side * 62}deg)`
          const artMissing = brokenArt.has(game.id)
          return (
            <Box
              key={game.id}
              id={`cover-${game.id}`}
              role="option"
              aria-selected={distance === 0}
              aria-label={`${game.title}, ${game.year}`}
              onClick={() => handleCoverClick(index)}
              sx={{
                position: 'absolute',
                top: compact ? 12 : 20,
                left: '50%',
                width: coverSize,
                height: coverSize,
                zIndex: games.length - distance,
                transform,
                transformStyle: 'preserve-3d',
                opacity: distance > VISIBLE_SIDE_COVERS ? 0 : 1,
                pointerEvents: distance > VISIBLE_SIDE_COVERS ? 'none' : 'auto',
                cursor: 'pointer',
                transition: reducedMotion ? 'none' : 'transform .45s cubic-bezier(.2,.8,.2,1), opacity .45s ease',
                // Side covers darken as they recede, like the original cover flow.
                filter: distance === 0 ? 'none' : `brightness(${Math.max(0.35, 0.8 - distance * 0.12)})`,
              }}
            >
              {artMissing ? (
                <Box sx={{ width: '100%', height: '100%', display: 'grid', placeContent: 'center', gap: 1, p: 2, textAlign: 'center', bgcolor: 'background.paper', border: 1, borderColor: 'divider' }}>
                  <Typography sx={{ font: '9px/1.8 var(--pixel)', color: 'primary.light' }}>{game.title}</Typography>
                  <Typography sx={{ font: '9px var(--mono)', color: 'text.secondary' }}>GAME BOY ADVANCE</Typography>
                </Box>
              ) : (
                <>
                  <Box
                    component="img"
                    src={coverUrl(game)}
                    alt=""
                    draggable={false}
                    loading="lazy"
                    onError={() => setBrokenArt((ids) => new Set(ids).add(game.id))}
                    sx={{ width: '100%', height: '100%', display: 'block', objectFit: 'cover', boxShadow: '0 18px 40px #000a' }}
                  />
                  {/* Mirrored copy fading out below the cover, the floor reflection of the iPod cover flow. */}
                  <Box
                    component="img"
                    src={coverUrl(game)}
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
          <Typography sx={{ color: 'text.secondary', font: '11px var(--mono)' }}>{current.year}</Typography>
          <Chip
            size="small"
            variant="outlined"
            color={inLibrary ? 'primary' : 'default'}
            label={inLibrary ? 'In your library' : 'Bring your own ROM'}
            sx={{ font: '10px var(--mono)', borderRadius: 1 }}
          />
        </Stack>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
          <IconButton aria-label="Previous cartridge" onClick={() => { setPaused(true); select(active - 1) }} disabled={active === 0}>
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
          <IconButton aria-label="Next cartridge" onClick={() => { setPaused(true); select(active + 1) }} disabled={active === games.length - 1}>
            <ChevronRight />
          </IconButton>
        </Stack>
        {/* The scrubber under the covers, as on the iPod. */}
        <Slider
          aria-label="Browse cartridges"
          size="small"
          min={0}
          max={games.length - 1}
          step={1}
          value={active}
          onChange={(_, value) => { setPaused(true); select(value as number) }}
          getAriaValueText={(value) => games[value]?.title ?? ''}
          sx={{ width: compact ? '70%' : 320, color: 'primary.main', '& .MuiSlider-thumb': { width: 10, height: 10, borderRadius: 0 } }}
        />
      </Stack>
    </Box>
  )
}
