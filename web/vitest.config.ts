import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

// Unit tests target pure modules in shared/ and server/utils/ (no Nuxt runtime needed → fast & deterministic).
export default defineConfig({
  resolve: {
    alias: {
      '#shared': fileURLToPath(new URL('./shared', import.meta.url)),
    },
  },
  test: {
    include: ['tests/unit/**/*.test.ts'],
    environment: 'node',
  },
})
