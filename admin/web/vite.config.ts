import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// 产物给 pointer-admin 进程用 ServeDir 挂到 /assets，所以 base 保持默认 '/'
export default defineConfig({
  plugins: [vue()],
  server: {
    port: 5173,
    proxy: {
      // 开发时把接口透传给控制面进程
      '/api': { target: 'http://127.0.0.1:8788', changeOrigin: true },
      '/oauth': { target: 'http://127.0.0.1:8788', changeOrigin: true },
      '/auth': { target: 'http://127.0.0.1:8788', changeOrigin: true },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
