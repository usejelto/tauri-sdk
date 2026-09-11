// Only the locally built SDK changes between example installs. Preserve every
// registry dependency and its integrity while recording this archive's bytes.
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'

const root = new URL('../', import.meta.url)
const sdk = JSON.parse(readFileSync(new URL('package.json', root), 'utf8'))
const example = JSON.parse(readFileSync(new URL('example/package.json', root), 'utf8'))
const lockPath = new URL('example/package-lock.json', root)
const lock = JSON.parse(readFileSync(lockPath, 'utf8'))
const archive = `${sdk.name.replace('@', '').replace('/', '-')}-${sdk.version}.tgz`
const dependency = `file:../${archive}`
const entry = lock.packages[`node_modules/${sdk.name}`]

if (example.dependencies[sdk.name] !== dependency ||
    lock.packages[''].dependencies[sdk.name] !== dependency ||
    entry?.resolved !== dependency || entry.version !== sdk.version) {
  throw new Error('Update the example manifest and lockfile to reference the current SDK version.')
}

entry.integrity = `sha512-${createHash('sha512').update(readFileSync(new URL(archive, root))).digest('base64')}`
writeFileSync(lockPath, `${JSON.stringify(lock, null, 2)}\n`)
console.log(`Example locked to the freshly built ${archive}`)
