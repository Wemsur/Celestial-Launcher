import vue from '@vitejs/plugin-vue'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'

import { celestialVueAlias } from './build/celestial-vue.mjs'

// Resolved from this config file's own URL: Vite rewrites `import.meta.url` in a
// bundled config to point at the original file, so this is the plugin project
// root no matter which directory `vite` was invoked from.
const projectRoot = fileURLToPath(new URL('.', import.meta.url))

export default defineConfig({
	plugins: [vue()],
	resolve: {
		// Redirects every `vue` import — including the ones the SFC compiler
		// emits — to a generated shim that reads the launcher's Vue off a
		// global. Without it the plugin would either carry a second Vue the
		// launcher cannot render, or ship a bare `import 'vue'` that fails at
		// load time.
		alias: [celestialVueAlias(projectRoot)],
	},
	build: {
		target: 'chrome105',
		// A plugin loads on its own and is not code-split, so it is one file with
		// no separate CSS asset — styles go through `api.styles.add`.
		lib: {
			entry: join(projectRoot, 'src/index.ts'),
			formats: ['es'],
			fileName: () => 'index.js',
		},
		// The bundle gets read by hand when something fails at load time.
		sourcemap: true,
		// Themes and small widgets read better unminified while debugging; turn
		// this on for a release build if you prefer.
		minify: false,
	},
})
