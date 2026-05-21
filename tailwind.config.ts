import type { Config } from 'tailwindcss'
import animate from 'tailwindcss-animate'

export default {
  darkMode: 'class',
  content: ['./index.html', './src/**/*.{vue,ts,tsx,js,jsx}'],
  theme: {
    extend: {
      fontFamily: {
        sans: ['Inter', 'PingFang SC', 'system-ui', 'sans-serif']
      },
      colors: {
        background: 'hsl(var(--background) / <alpha-value>)',
        foreground: 'hsl(var(--foreground) / <alpha-value>)',
        card: 'hsl(var(--card) / <alpha-value>)',
        muted: 'hsl(var(--muted) / <alpha-value>)',
        border: 'hsl(var(--border) / <alpha-value>)',
        accent: {
          DEFAULT: 'hsl(var(--accent) / <alpha-value>)',
          muted: 'hsl(var(--accent-muted) / <alpha-value>)'
        },
        /** Legacy alias → accent (P0) */
        primary: {
          DEFAULT: 'hsl(var(--accent) / <alpha-value>)',
          cyan: 'hsl(var(--accent) / <alpha-value>)',
          fuchsia: 'hsl(var(--accent) / <alpha-value>)'
        },
        success: 'hsl(var(--success) / <alpha-value>)',
        danger: 'hsl(var(--danger) / <alpha-value>)',
        warning: 'hsl(var(--warning) / <alpha-value>)',
        info: 'hsl(var(--info) / <alpha-value>)',
        hover: 'hsl(var(--hover) / <alpha-value>)'
      },
      keyframes: {
        pulseDot: {
          '0%,100%': { opacity: '0.35' },
          '50%': { opacity: '1' }
        }
      },
      animation: {
        pulseDot: 'pulseDot 1.4s ease-in-out infinite'
      }
    }
  },
  plugins: [animate]
} satisfies Config
