import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
import { VitePWA } from 'vite-plugin-pwa'

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: 'autoUpdate',
      includeAssets: ['logo.png', 'icons.svg'],
      manifest: {
        name: 'GBA WASM',
        short_name: 'GBAWASM',
        description: 'Retro gaming emulator shell built with Rust and WebAssembly.',
        theme_color: '#19171e',
        background_color: '#19171e',
        display: 'standalone',
        start_url: '/',
        icons: [
          {
            src: '/logo.png',
            sizes: '1313x1198',
            type: 'image/png',
            purpose: 'any maskable',
          },
        ],
      },
    }),
  ],
})
