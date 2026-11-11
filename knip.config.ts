// Knip for the published TypeScript bindings in guest-js/ (`make deadcode`):
// files nothing imports, unused exports and unused or unlisted npm
// dependencies. example/ is a separate application with its own package.json,
// and scripts/ holds a build helper the Makefile runs.
import type { KnipConfig } from 'knip'

const config: KnipConfig = {
  // Knip derives the package entry, guest-js/index.ts, from package.json
  // `exports` and tsconfig.json; Vitest's plugin adds the *.test.ts files.
  project: ['guest-js/**/*.ts'],
  // The Makefile's format-check target runs Prettier through npx, and Knip
  // does not read the Makefile.
  ignoreDependencies: ['prettier'],
}

export default config
