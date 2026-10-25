import { useEffect, useRef, useState, type ChangeEvent } from 'react'
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

const THUMB_DEMO_ROM = Uint8Array.from([
  0x01, 0x48, 0x1f, 0x21, 0x01, 0x60, 0xfe, 0xe7, 0x00, 0x00, 0x00, 0x06,
])
const ARM_BOOT_DEMO_ROM = Uint8Array.from([
  0x00, 0x00, 0x00, 0xea, 0x00, 0x00, 0xa0, 0xe1, 0x2a, 0x00, 0xa0, 0xe3,
])

const theme = createTheme({
  shape: { borderRadius: 0 },
  components: {
    MuiChip: {
      styleOverrides: {
        root: { borderRadius: 0 },
      },
    },
  },
  palette: {
    mode: 'dark',
    primary: { main: '#5b5b8a', contrastText: '#ffffff' },
    background: { default: '#0f172a', paper: '#111827' },
  },
  typography: {
    fontFamily: '"Press Start 2P", monospace',
  },
})

function App() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const fileInputRef = useRef<HTMLInputElement | null>(null)
  const [status, setStatus] = useState('Loading WASM…')
  const [ready, setReady] = useState(false)

  useEffect(() => {
    let cancelled = false

    async function initialize() {
      try {
        const wasm = await loadGbaWasm()
        if (!cancelled) {
          setStatus(wasm.hello('GBA'))
          setReady(true)
        }
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

  async function runProgram(rom: Uint8Array, name: string, steps = 1000, thumb = false) {
    try {
      setStatus(`Running ${name}...`)
      const wasm = await loadGbaWasm()
      const pixels = thumb
        ? wasm.run_thumb_rom(rom, steps)
        : wasm.run_rom(rom, steps)
      const context = canvasRef.current?.getContext('2d')
      if (!context) throw new Error('Canvas rendering is unavailable')

      const imagePixels = new Uint8ClampedArray(pixels)
      context.putImageData(new ImageData(imagePixels, 240, 160), 0, 0)
      setStatus(`${name}: executed ${steps} instructions`)
      setReady(true)
    } catch (error) {
      setStatus(error instanceof Error ? error.message : 'Thumb program failed')
    }
  }

  async function loadSelectedProgram(event: ChangeEvent<HTMLInputElement>) {
    const file = event.currentTarget.files?.[0]
    event.currentTarget.value = ''
    if (!file) return

    await runProgram(new Uint8Array(await file.arrayBuffer()), file.name)
  }

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <AppBar position="sticky" color="transparent" elevation={0}>
        <Toolbar sx={{ justifyContent: 'space-between' }}>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5 }}>
            <Box
              component="img"
              src="/logo.png"
              alt=""
              sx={{ width: 36, height: 36, objectFit: 'contain', imageRendering: 'pixelated' }}
            />
            <Typography component="div" variant="h6" sx={{ fontWeight: 700 }}>
              GBA WASM
            </Typography>
          </Box>
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
          <Paper sx={{ p: { xs: 2, md: 3 } }}>
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
                <Button
                  variant="contained"
                  color="primary"
                  onClick={() => fileInputRef.current?.click()}
                >
                  Load GBA ROM
                </Button>
                <Button
                  variant="outlined"
                  color="secondary"
                  onClick={() => void runProgram(ARM_BOOT_DEMO_ROM, 'ARM boot demo', 2)}
                >
                  Run ARM Boot Demo
                </Button>
                <Button
                  variant="outlined"
                  color="secondary"
                  onClick={() => void runProgram(THUMB_DEMO_ROM, 'Thumb demo', 3, true)}
                >
                  Run Thumb Demo
                </Button>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept=".gba,.bin,application/octet-stream"
                  onChange={(event) => void loadSelectedProgram(event)}
                  hidden
                />
              </Box>
            </Box>
          </Paper>

          <Paper
            elevation={0}
            sx={{
              p: 2,
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
                }}
              />
            </Box>
          </Paper>

          <Paper sx={{ p: 2 }}>
            <Typography variant="body2" color="text.secondary">
              ROMs start at 0x08000000 using an HLE ARM entry. CPU, BIOS, timing, and GPU support remain incomplete; most retail ROMs will not boot yet.
            </Typography>
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
