import { createTheme } from '@mui/material/styles'

/** MUI theme matching the palette and fonts in index.css. */
export const muiTheme = createTheme({
  palette: {
    mode: 'dark',
    primary: { main: '#bb8df2', light: '#d9b7ff', contrastText: '#1a1420' },
    secondary: { main: '#e47d4e' },
    background: { default: '#19171e', paper: '#232027' },
    text: { primary: '#e8e8e3', secondary: '#aaa4b0' },
    divider: '#3d3844',
  },
  shape: { borderRadius: 0 },
  typography: {
    fontFamily: "'Manrope', sans-serif",
    button: { fontFamily: "'Press Start 2P', monospace", fontSize: 8, textTransform: 'none' },
  },
})
