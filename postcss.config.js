import tailwindcss from 'tailwindcss'
import autoprefixer from 'autoprefixer'
import legacyCssCompat from './scripts/lib/postcss-legacy-compat.mjs'

export default {
  plugins: [
    tailwindcss(),
    autoprefixer(),
    // Runs last so it sees the final Tailwind output. Downgrades the handful of
    // selectors/shorthands Chrome 63/66 would otherwise drop entirely.
    legacyCssCompat()
  ]
}
