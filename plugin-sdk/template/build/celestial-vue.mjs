/**
 * Resolve a plugin bundle's `vue` imports to the launcher's Vue.
 *
 * A plugin runs inside the launcher's page but is loaded as a standalone
 * module, so it cannot see the launcher's bundled Vue, and bundling its own
 * copy would hand it a different Vue instance whose components the launcher
 * cannot render. This plugin replaces the `vue` module with a shim that
 * re-exports from the global the launcher publishes (`__CELESTIAL_PLUGIN_VUE__`).
 *
 * The shim's named exports are generated from the developer's installed `vue`,
 * so `.vue` single-file components — whose compiled output imports a wide set
 * of runtime helpers — resolve every symbol against the launcher's instance.
 */

import { createRequire } from 'node:module'

const VIRTUAL_ID = '\0celestial:vue'
const GLOBAL_KEY = '__CELESTIAL_PLUGIN_VUE__'

export function celestialVue() {
	return {
		name: 'celestial-vue-external',
		enforce: 'pre',
		resolveId(id) {
			if (id === 'vue') {
				return VIRTUAL_ID
			}
			return null
		},
		load(id) {
			if (id !== VIRTUAL_ID) {
				return null
			}

			const require = createRequire(import.meta.url)
			// The installed `vue` is a devDependency only — its export names are
			// read here so the shim matches whatever version the developer built
			// against, which is expected to be the launcher's Vue 3.x line.
			const vue = require('vue')
			const names = Object.keys(vue).filter((name) =>
				/^[A-Za-z_$][\w$]*$/.test(name),
			)

			const lines = [
				`const runtime = globalThis[${JSON.stringify(GLOBAL_KEY)}];`,
				`if (!runtime) {`,
				`  throw new Error(`,
				`    'Celestial: the launcher did not expose Vue. This plugin must run inside Celestial Launcher.',`,
				`  );`,
				`}`,
				`export default runtime;`,
			]
			for (const name of names) {
				if (name === 'default') {
					continue
				}
				lines.push(`export const ${name} = runtime[${JSON.stringify(name)}];`)
			}
			return lines.join('\n')
		},
	}
}
