import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'GBA WASM',
  description: 'The official documentation for the GBA emulator on WebAssembly.',
  lang: 'en-US',
  base: '/gba-wasm/docs/',
  lastUpdated: true,
  vite: {
    publicDir: '../public',
  },
  themeConfig: {
    logo: '/logo.png',
    nav: [
      { text: 'User Manual', link: '/manual/' },
      { text: 'Guide', link: '/guide/introduction' },
      { text: 'Architecture', link: '/architecture/overview' },
      { text: 'Rust Core', link: '/rust-core/memory-map' },
    ],
    sidebar: [
      {
        text: 'User Manual',
        items: [
          { text: 'Getting Started', link: '/manual/' },
          { text: 'Saved Games', link: '/manual/saves' },
          { text: 'Settings', link: '/manual/settings' },
          { text: 'Desktop and Mobile Play', link: '/manual/play-modes' },
          { text: 'Controls and Shortcuts', link: '/manual/controls' },
        ],
      },
      {
        text: 'Development',
        items: [
          { text: 'Introduction', link: '/guide/introduction' },
          { text: 'Project Goals', link: '/guide/goals' },
          { text: 'Development Workflow', link: '/guide/workflow' },
        ],
      },
      {
        text: 'Architecture',
        items: [
          { text: 'Overview', link: '/architecture/overview' },
          { text: 'Memory Model', link: '/architecture/memory-model' },
          { text: 'GPU Pipeline', link: '/architecture/gpu-pipeline' },
          { text: 'WASM Boundary', link: '/architecture/wasm-boundary' },
        ],
      },
      {
        text: 'Rust Core',
        items: [
          { text: 'Memory Map', link: '/rust-core/memory-map' },
          { text: 'ARM7TDMI CPU', link: '/rust-core/thumb-cpu' },
          { text: 'HLE BIOS', link: '/rust-core/bios' },
          { text: 'GPU Core', link: '/rust-core/gpu-core' },
          { text: 'Audio (APU)', link: '/rust-core/apu' },
          { text: 'Testing Strategy', link: '/rust-core/testing-strategy' },
        ],
      },
    ],
    socialLinks: [{ icon: 'github', link: 'https://github.com/smolgroot/gba-wasm' }],
  },
})
