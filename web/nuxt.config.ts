import pkg from './package.json' with { type: 'json' }

// Fraud Detection Platform — web UI (Nuxt 4 SSR + BFF).
// The browser only talks to this Nuxt server; the Nitro server routes (server/) hold the JWTs in a sealed
// httpOnly session cookie and forward /api/** to the gateway. See README.md.
export default defineNuxtConfig({

  modules: ['@nuxt/ui', '@nuxt/eslint', '@pinia/nuxt', 'nuxt-auth-utils', '@nuxtjs/i18n'],

  // Components are organised in folders (common/, rules/, ml/…) but referenced by file name.
  components: [{ path: '~/components', pathPrefix: false }],
  devtools: { enabled: false },

  css: ['~/assets/css/main.css'],

  runtimeConfig: {
    // Server-only. Every key can be overridden at runtime with NUXT_<UPPER_SNAKE> env vars.
    apiBaseUrl: 'http://localhost:8080', // NUXT_API_BASE_URL — the gateway
    apiTimeoutMs: 30_000, // NUXT_API_TIMEOUT_MS (streams are exempt)
    mockApi: '', // NUXT_MOCK_API=1 (or MOCK_API=1) → serve fixtures, no backend needed
    sessionSecret: '', // NUXT_SESSION_SECRET (compose name) → copied into session.password at startup
    sessionSecure: '', // NUXT_SESSION_SECURE=true behind TLS
    session: {
      name: 'fraud_session',
      password: '', // NUXT_SESSION_PASSWORD also accepted (nuxt-auth-utils default)
      maxAge: 60 * 60 * 24 * 14,
      cookie: { sameSite: 'strict', httpOnly: true, secure: false },
    },
    public: {
      appName: 'Fraud Platform',
      appVersion: pkg.version, // NUXT_PUBLIC_APP_VERSION overrides (e.g. image tag)
      sourceUrl: 'https://github.com/situkangsayur/fraud_detection_engine',
    },
  },

  routeRules: {
    '/api/**': { headers: { 'cache-control': 'no-store' } },
  },
  compatibilityDate: '2026-09-01',

  nitro: {
    compressPublicAssets: true,
  },

  vite: {
    optimizeDeps: {
      include: ['echarts/core', 'echarts/charts', 'echarts/components', 'echarts/renderers', 'vue-echarts', 'cytoscape', 'cytoscape-fcose', 'marked', 'dompurify'],
    },
  },

  typescript: {
    strict: true,
    typeCheck: false, // run explicitly: npm run typecheck
  },

  eslint: {
    config: { stylistic: { semi: false, quotes: 'single', commaDangle: 'always-multiline' } },
  },

  i18n: {
    defaultLocale: 'id',
    strategy: 'no_prefix',
    locales: [
      { code: 'id', language: 'id-ID', name: 'Bahasa Indonesia', file: 'id.json' },
      { code: 'en', language: 'en-US', name: 'English', file: 'en.json' },
    ],
    detectBrowserLanguage: { useCookie: true, cookieKey: 'fraud_locale', redirectOn: 'root' },
  },

  icon: {
    // Icons are bundled from local @iconify-json packages → no runtime calls to the Iconify API (air-gapped friendly).
    serverBundle: { collections: ['lucide'] },
    clientBundle: { scan: true, sizeLimitKb: 512 },
    provider: 'server',
  },
})
