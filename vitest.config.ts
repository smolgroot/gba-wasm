import { defineConfig } from 'vitest/config'

// Kept apart from vite.config.ts so tests do not load the PWA plugin.
export default defineConfig({
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
  },
})
