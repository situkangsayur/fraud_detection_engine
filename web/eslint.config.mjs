// @ts-check
import withNuxt from './.nuxt/eslint.config.mjs'

export default withNuxt({
  rules: {
    'vue/multi-word-component-names': 'off',
    'vue/no-v-html': 'off', // only used by MarkdownView, which sanitises with DOMPurify
    '@typescript-eslint/no-explicit-any': 'warn',
    // Short guard clauses like `try { a() } catch { b() }` stay on one line; longer chains must be split.
    '@stylistic/max-statements-per-line': ['error', { max: 4 }],
  },
}, {
  ignores: ['.output/**', '.nuxt/**', 'node_modules/**', 'coverage/**'],
})
