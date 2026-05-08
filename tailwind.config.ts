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
        primary: {
          DEFAULT: '#7C3AED',
          cyan: '#06B6D4',
          fuchsia: '#A855F7'
        },
        success: '#22C55E',
        danger: '#EF4444',
        warning: '#F59E0B',
        info: '#38BDF8'
      },
      backgroundImage: {
        'aurora':
          'radial-gradient(60% 50% at 20% 10%, rgba(124,58,237,0.25), transparent 60%), radial-gradient(50% 40% at 80% 20%, rgba(6,182,212,0.18), transparent 60%), radial-gradient(60% 50% at 50% 100%, rgba(168,85,247,0.18), transparent 70%)'
      },
      keyframes: {
        shimmer: {
          '0%': { backgroundPosition: '-400px 0' },
          '100%': { backgroundPosition: '400px 0' }
        },
        pulseDot: {
          '0%,100%': { opacity: '0.35' },
          '50%': { opacity: '1' }
        }
      },
      animation: {
        shimmer: 'shimmer 2.4s linear infinite',
        pulseDot: 'pulseDot 1.4s ease-in-out infinite'
      }
    }
  },
  plugins: [animate]
} satisfies Config
