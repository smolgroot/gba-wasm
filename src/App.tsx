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
const NINTENDO_LOGO = Uint8Array.from([
  0x24, 0xff, 0xae, 0x51, 0x69, 0x9a, 0xa2, 0x21, 0x3d, 0x84, 0x82, 0x0a, 0x84, 0xe4,
  0x09, 0xad, 0x11, 0x24, 0x8b, 0x98, 0xc0, 0x81, 0x7f, 0x21, 0xa3, 0x52, 0xbe, 0x19,
  0x93, 0x09, 0xce, 0x20, 0x10, 0x46, 0x4a, 0x4a, 0xf8, 0x27, 0x31, 0xec, 0x58, 0xc7,
  0xe8, 0x33, 0x82, 0xe3, 0xce, 0xbf, 0x85, 0xf4, 0xdf, 0x94, 0xce, 0x4b, 0x09, 0xc1,
  0x94, 0x56, 0x8a, 0xc0, 0x13, 0x72, 0xa7, 0xfc, 0x9f, 0x84, 0x4d, 0x73, 0xa3, 0xca,
  0x9a, 0x61, 0x58, 0x97, 0xa3, 0x27, 0xfc, 0x03, 0x98, 0x76, 0x23, 0x1d, 0xc7, 0x61,
  0x03, 0x04, 0xae, 0x56, 0xbf, 0x38, 0x84, 0x00, 0x40, 0xa7, 0x0e, 0xfd, 0xff, 0x52,
  0xfe, 0x03, 0x6f, 0x95, 0x30, 0xf1, 0x97, 0xfb, 0xc0, 0x85, 0x60, 0xd6, 0x80, 0x25,
  0xa9, 0x63, 0xbe, 0x03, 0x01, 0x4e, 0x38, 0xe2, 0xf9, 0xa2, 0x34, 0xff, 0xbb, 0x3e,
  0x03, 0x44, 0x78, 0x00, 0x90, 0xcb, 0x88, 0x11, 0x3a, 0x94, 0x65, 0xc0, 0x7c, 0x63,
  0x87, 0xf0, 0x3c, 0xaf, 0xd6, 0x25, 0xe4, 0x8b, 0x38, 0x0a, 0xac, 0x72, 0x21, 0xd4,
  0xf8, 0x07,
])

function makeArmBootDemoRom() {
  const rom = new Uint8Array(0xc4)
  rom.set([0x2e, 0x00, 0x00, 0xea])
  rom.set(NINTENDO_LOGO, 4)
  rom.set(new TextEncoder().encode('SYNTHETIC   '), 0xa0)
  rom.set(new TextEncoder().encode('TEST'), 0xac)
  rom.set(new TextEncoder().encode('00'), 0xb0)
  rom[0xb2] = 0x96
  const headerSum = rom.subarray(0xa0, 0xbd).reduce((sum, byte) => (sum + byte) & 0xff, 0)
  rom[0xbd] = (-headerSum - 0x19) & 0xff
  rom.set([0x2a, 0x00, 0xa0, 0xe3], 0xc0)
  return rom
}

const ARM_BOOT_DEMO_ROM = makeArmBootDemoRom()

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
      const cartridgeInfo = thumb ? '' : ` (${wasm.inspect_rom(rom)})`
      const pixels = thumb
        ? wasm.run_thumb_rom(rom, steps)
        : wasm.run_rom(rom, steps)
      const context = canvasRef.current?.getContext('2d')
      if (!context) throw new Error('Canvas rendering is unavailable')

      const imagePixels = new Uint8ClampedArray(pixels)
      context.putImageData(new ImageData(imagePixels, 240, 160), 0, 0)
      setStatus(`${name}${cartridgeInfo}: executed ${steps} instructions`)
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
