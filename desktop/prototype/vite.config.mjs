import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import react from '@vitejs/plugin-react';
import {defineConfig} from 'vite';

const root = fileURLToPath(new URL('../', import.meta.url));
const samples = [
  'agent-page-design/index', 'agent-picker-design/index', 'agent-picker-design/compact',
  'agent-picker-design/grid', 'agent-picker-design/detail', 'settings-navigation-design/index',
  'agent-settings-polish', 'automation-interface', 'computer-use-plugin', 'console-pages',
  'editor-loading', 'loading-motion', 'orca-light-workbench', 'orchestration', 'orchestration-v2',
  'page-navigation', 'permission-host-qa', 'plugins', 'project-shell', 'retired-session',
  'settings-extension', 'terminal-input-parity', 'terminal-load', 'terminal-white-surface',
  'workbench-interface', 'workbench-terminal-ui', 'workspace-options',
].map(path => 'tests/' + path + '.html');

export default defineConfig({
  root,
  base: '/',
  plugins: [react(), {
    name: 'prototype-page-isolation',
    transformIndexHtml: {
      order: 'pre',
      handler(html, context) {
        const page = html.replace(/<base href="\/">/g, '');
        return context.path.endsWith('/prototype/index.html') ? page : page.replace(
          '<head>',
          '<head><script type="module" src="/prototype/isolation.ts"></script>',
        );
      },
    },
  }],
  build: {
    emptyOutDir: false,
    target: 'es2022',
    chunkSizeWarningLimit: 5000,
    rollupOptions: {
      input: ['prototype/index.html', 'prototype/app.html', 'prototype/hud.html', ...samples].map(path => resolve(root, path)),
    },
  },
});
