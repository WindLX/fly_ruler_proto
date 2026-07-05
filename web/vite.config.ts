import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

const runtimeConfigPlugin = (command: string) => ({
  name: 'fly-ruler-runtime-config',
  transformIndexHtml(html: string) {
    if (command !== 'serve') return html
    return html.replace(
      '__FLY_RULER_RUNTIME_CONFIG__',
      JSON.stringify({
        api_base_url: '/api/v1',
        websocket_url: '/api/v1/ws',
      }),
    )
  },
})

export default defineConfig(({ command }) => ({
  plugins: [vue(), tailwindcss(), runtimeConfigPlugin(command)],
  resolve: {
    alias: {
      '@': path.resolve(path.dirname(fileURLToPath(import.meta.url)), './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8081',
        changeOrigin: true,
        ws: true,
      },
    },
  },
  test: {
    environment: 'node',
  },
}))
