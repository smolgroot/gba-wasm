import { useEffect, useRef, useState } from 'react'
import {
  AppBar,
  Box,
  Button,
  Chip,
  Container,
  CssBaseline,
  Paper,
  ThemeProvider,
  Toolbar,
  Typography,
  createTheme,
} from '@mui/material'
import { loadGbaWasm } from './wasmBridge'

const theme = createTheme({
  palette: {
    mode: 'dark',
    primary: { main: '#7c3aed' },
    background: { default: '#0f172a', paper: '#111827' },
  },
  typography: {
    fontFamily: 'Inter, system-ui, sans-serif',
  },
})

function App() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const [status, setStatus] = useState('Loading WASM…')
  const [ready, setReady] = useState(false)

  useEffect(() => {
    let cancelled = false

    async function initialize() {
      try {
        const wasm = await loadGbaWasm()
        const message = wasm.hello('GBA')

        if (!cancelled) {
          setStatus(message)
          setReady(true)
        }

        const canvas = canvasRef.current
        if (!canvas) return

        const context = canvas.getContext('2d')
        if (!context) return

        const width = 240
        const height = 160
        const pixels = new Uint8ClampedArray(wasm.make_test_frame(width, height))
        const image = new ImageData(pixels, width, height)

        context.putImageData(image, 0, 0)
      } catch (error) {
        if (!cancelled) {
          setStatus(error instanceof Error ? error.message : 'WASM failed to load')
        }
      }
    }

    void initialize()

    return () => {
      cancelled = true
    }
  }, [])

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <AppBar position="sticky" color="transparent" elevation={0}>
        <Toolbar sx={{ justifyContent: 'space-between' }}>
          <Typography component="div" variant="h6" sx={{ fontWeight: 700 }}>
            GBA WASM
          </Typography>
          <Chip
            label={ready ? 'Ready' : 'Booting'}
            size="small"
            color={ready ? 'success' : 'warning'}
            variant="filled"
          />
        </Toolbar>
      </AppBar>

      <Container maxWidth="xl" sx={{ py: 4 }}>
        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
          <Paper sx={{ p: { xs: 2, md: 3 }, borderRadius: 3 }}>
            <Box
              sx={{
                display: 'flex',
                flexDirection: { xs: 'column', md: 'row' },
                justifyContent: 'space-between',
                alignItems: { xs: 'flex-start', md: 'center' },
                gap: 2,
              }}
            >
              <Box>
                <Typography variant="overline" color="text.secondary">
                  Emulation shell
                </Typography>
                <Typography component="div" variant="h4" sx={{ fontWeight: 700 }}>
                  Retro gaming, compiled to WebAssembly.
                </Typography>
              </Box>

              <Box sx={{ display: 'flex', gap: 1, flexWrap: 'wrap' }}>
                <Button variant="contained" color="primary">
                  Load ROM
                </Button>
                <Button variant="outlined" color="secondary">
                  Pause
                </Button>
              </Box>
            </Box>
          </Paper>

          <Paper
            elevation={0}
            sx={{
              p: 2,
              borderRadius: 3,
              border: '1px solid',
              borderColor: 'divider',
              background: 'radial-gradient(circle at top, #1f2937 0%, #0f172a 55%)',
            }}
          >
            <Box
              sx={{
                display: 'flex',
                justifyContent: 'center',
                alignItems: 'center',
                background: '#020817',
                borderRadius: 2,
                minHeight: { xs: 220, md: 380 },
                overflow: 'hidden',
              }}
            >
              <canvas
                ref={canvasRef}
                width={240}
                height={160}
                style={{
                  width: '100%',
                  maxWidth: 720,
                  height: 'auto',
                  imageRendering: 'pixelated',
                  borderRadius: 8,
                }}
              />
            </Box>
          </Paper>

          <Paper sx={{ p: 2, borderRadius: 3 }}>
            <Typography variant="body2" color="text.secondary">
              Runtime status
            </Typography>
            <Typography variant="body1" sx={{ mt: 1 }}>
              {status}
            </Typography>
          </Paper>
        </Box>
      </Container>
    </ThemeProvider>
  )
}

export default App
