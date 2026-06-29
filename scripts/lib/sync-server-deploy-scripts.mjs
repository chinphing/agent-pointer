import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(__dirname, '../..')
const SCRIPTS_SRC = path.join(ROOT, 'server', 'scripts')

/** Copy start/stop/restart/status scripts next to the release binary. */
export function syncServerDeployScripts(destDir) {
  if (!fs.existsSync(SCRIPTS_SRC)) return
  fs.mkdirSync(destDir, { recursive: true })
  for (const name of fs.readdirSync(SCRIPTS_SRC)) {
    const src = path.join(SCRIPTS_SRC, name)
    if (!fs.statSync(src).isFile()) continue
    const dest = path.join(destDir, name)
    fs.copyFileSync(src, dest)
    if (name.endsWith('.sh')) {
      fs.chmodSync(dest, 0o755)
    }
  }
}

export const SERVER_SCRIPTS_SRC = SCRIPTS_SRC
