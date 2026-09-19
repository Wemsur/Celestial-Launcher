import vue from '@vitejs/plugin-vue'
import { resolve } from 'node:path'
import { defineConfig } from 'vite'

import { celestialVue } from './build/celestial-vue.mjs'

// Builds the plugin into a single ES module the launcher loads at runtime.
//
// - `celestialVue()` must come before `vue()` so it claims every `vue` import,
//   including the ones the SFC compiler emits, and points them at the
//   launcher's Vue instead of bundling a second Vue.
// - The output is one self-contained `dist/index.js`; the manifest's `entry`
//   points at it.
export default defineConfig({
	plugins: [celestialVue(), vue()],
	build: {
		target: 'chrome105',
		// A plugin is loaded on its own, not code-split, so keep it to one file
		// with styles applied through `api.styles.add` rather than a CSS asset.
		lib: {
			entry: resolve(__dirname, 'src/index.ts'),
			formats: ['es'],
			fileName: () => 'index.js',
		},
		rollupOptions: {
			// `vue` must NOT be external: celestialVue() rewrites it to a tiny
			// inlined shim that reads the launcher's Vue off the global. Marking
			// it external instead would leave a bare `import ... from 'vue'` in
			// the output, which a standalone plugin module cannot resolve at
			// runtime — exactly the error a plugin then throws on load.
			output: {
				inlineDynamicImports: true,
			},
		},
		// Themes and small widgets read better unminified while debugging; flip
		// this on for a release build if you want.
		minify: false,
	},
})
