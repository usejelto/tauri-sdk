// ESLint for the published TypeScript bindings in guest-js/ (`make lint`).
// The Rust crate has Clippy, and example/ is a separate application with its
// own dependencies and lockfile, so only guest-js/ is in scope.
import js from '@eslint/js'
import { defineConfig } from 'eslint/config'
import globals from 'globals'
import tseslint from 'typescript-eslint'

export default defineConfig(
  // Everything outside guest-js/ is ignored, so a new file elsewhere stays out
  // of scope until it is named here.
  { ignores: ['**', '!guest-js/', '!guest-js/**'] },
  {
    files: ['guest-js/**/*.ts'],
    // The recommended rule sets of ESLint and typescript-eslint, unchanged.
    extends: [js.configs.recommended, tseslint.configs.recommended],
    // The bindings run in the application's webview.
    languageOptions: { globals: globals.browser },
  },
  {
    // The tests run under Vitest in Node.
    files: ['guest-js/**/*.test.ts'],
    languageOptions: { globals: globals.node },
  },
)
