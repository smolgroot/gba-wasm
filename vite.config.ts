import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
import { VitePWA } from 'vite-plugin-pwa'

const base = '/gba-wasm/'

export default defineConfig({
  base,
  plugins: [
    react(),
    VitePWA({
      registerType: 'autoUpdate',
      workbox: {
        navigateFallbackDenylist: [/^\/gba-wasm\/docs(?:\/|$)/],
      },
      includeAssets: [
        'favicon.svg',
        'logo.png',
        'icons.svg',
        'icons/icon-192.png',
        'icons/icon-512.png',
      ],
      manifest: {
        name: 'GBA WASM',
        short_name: 'GBAWASM',
        description: 'Retro gaming emulator shell built with Rust and WebAssembly.',
        id: base,
        theme_color: '#19171e',
        background_color: '#19171e',
        display: 'fullscreen',
        display_override: ['fullscreen', 'standalone'],
        start_url: base,
        scope: base,
        icons: [
          {
            src: `${base}icons/icon-192.png`,
            sizes: '192x192',
            type: 'image/png',
            purpose: 'any maskable',
          },
          {
            src: `${base}icons/icon-512.png`,
            sizes: '512x512',
            type: 'image/png',
            purpose: 'any maskable',
          },
        ],
      },
    }),
  ],
})
