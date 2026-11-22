import {defineConfig} from 'vite';
import react from '@vitejs/plugin-react';
import {fileURLToPath} from 'node:url';

// Visual-only server: no desktop runtime bridge or production entry is loaded.
export default defineConfig({
  root:fileURLToPath(new URL('../..',import.meta.url)),
  plugins:[react()],
  server:{host:'127.0.0.1',port:8816,strictPort:true},
});
