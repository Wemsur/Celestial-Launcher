/**
 * Loads UI plugins into the launcher's own page.
 *
 * Everything a plugin is given goes through {@link createHostApi}, which closes
 * over that plugin's id and its granted list. A plugin never supplies its own
 * id, so it cannot address another plugin's data, and a capability it was not
 * granted throws a named error rather than being silently absent — the author
 * gets told what to declare instead of debugging `undefined`.
 *
 * What this cannot do is isolate a plugin that ignores the API entirely: it
 * runs in the launcher's own JavaScript context, so it can reach the same
 * globals the launcher can. The host API is the intended path, not a sandbox.
 */

import {
	listPlugins,
	pluginAssetUrl,
	pluginReadEntry,
	pluginReportCrash,
	pluginStorageGet,
	pluginStorageKeys,
	pluginStorageRemove,
	pluginStorageSet,
} from './ipc'
import type { PluginSummary } from './types'

/**
 * Places a plugin may attach UI to. Fixed here rather than free-form because
 * the id is interpolated into a CSS selector and because the launcher only
 * renders containers for the slots it knows about.
 */
export const PLUGIN_SLOTS = [
	'topbar.left',
	'topbar.center',
	'topbar.right',
	'navbar.bottom',
	'sidebar.top',
	'sidebar.bottom',
] as const

export type PluginSlotId = (typeof PLUGIN_SLOTS)[number]

export interface SlotDefinition {
	id: string
	render: () => Node | null | void
}

export interface PluginHostApi {
	readonly plugin: {
		readonly id: string
		readonly name: string
		readonly version: string
	}
	readonly styles: {
		add(css: string): void
	}
	readonly slots: {
		add(slot: PluginSlotId, definition: SlotDefinition): void
	}
	readonly storage: {
		get(key: string): Promise<string | null>
		set(key: string, value: string): Promise<void>
		remove(key: string): Promise<void>
		keys(): Promise<string[]>
	}
	log(...args: unknown[]): void
}

interface PluginModule {
	activate?: (api: PluginHostApi) => void | Promise<void>
	default?: {
		activate?: (api: PluginHostApi) => void | Promise<void>
	}
}

interface MountRecord {
	pluginId: string
	key: string
	slotId: PluginSlotId
	element: HTMLElement
	container: HTMLElement | null
}

export interface LoadPluginsOptions {
	/** Called after a plugin threw and the launcher switched it off. */
	onCrash?: (summary: PluginSummary, message: string) => void
}

const instances = new Map<string, PluginSummary>()
const mounts: MountRecord[] = []
let observer: MutationObserver | null = null

export async function loadPlugins(options: LoadPluginsOptions = {}): Promise<void> {
	const summaries = await listPlugins()
	for (const summary of summaries) {
		try {
			await loadPlugin(summary, options)
		} catch (error) {
			// A failure while loading one plugin must not stop the others.
			await reportCrash(summary, error, options)
		}
	}
}

export function unloadPlugin(pluginId: string): void {
	for (let index = mounts.length - 1; index >= 0; index -= 1) {
		if (mounts[index].pluginId === pluginId) {
			mounts[index].element.remove()
			mounts.splice(index, 1)
		}
	}
	for (const element of document.querySelectorAll(`style[data-plugin="${cssEscape(pluginId)}"]`)) {
		element.remove()
	}
	instances.delete(pluginId)
}

export function unloadPlugins(): void {
	for (const pluginId of [...instances.keys()]) {
		unloadPlugin(pluginId)
	}
}

async function loadPlugin(
	summary: PluginSummary,
	options: LoadPluginsOptions,
): Promise<void> {
	if (instances.has(summary.id) || !summary.enabled) {
		return
	}
	const manifest = summary.manifest
	if (!manifest || summary.error || manifest.type !== 'ui' || !manifest.entry) {
		return
	}

	const activate = await importActivate(summary, manifest.entry)
	if (!activate) {
		throw new Error('The plugin does not export an activate() function.')
	}

	instances.set(summary.id, summary)
	try {
		await activate(createHostApi(summary))
	} catch (error) {
		unloadPlugin(summary.id)
		throw error
	}
}

/**
 * Import the entry bundle and hand back its `activate` export.
 *
 * The asset protocol is tried first because it streams the file. It is a
 * cross-origin module load, though, so CORS or CSP can refuse it; the entry is
 * then read through IPC and imported from a blob URL instead.
 */
async function importActivate(
	summary: PluginSummary,
	entry: string,
): Promise<PluginModule['activate'] | undefined> {
	const url = pluginAssetUrl(joinPath(summary.path, entry))
	try {
		const module = (await import(/* @vite-ignore */ url)) as PluginModule
		return module.activate ?? module.default?.activate
	} catch (assetError) {
		const source = await pluginReadEntry(summary.id)
		if (source === null) {
			throw assetError
		}
		const blobUrl = URL.createObjectURL(
			new Blob([source], { type: 'text/javascript' }),
		)
		try {
			const module = (await import(/* @vite-ignore */ blobUrl)) as PluginModule
			return module.activate ?? module.default?.activate
		} finally {
			URL.revokeObjectURL(blobUrl)
		}
	}
}

function createHostApi(summary: PluginSummary): PluginHostApi {
	const granted = new Set(summary.granted)
	const pluginId = summary.id

	// Scoped permissions are stored with their scope (`slot:sidebar.top`), so a
	// capability is present when *any* permission of that kind was granted — an
	// exact `has('slot')` never matches and would deny a correctly declared
	// plugin. The scope itself is checked where it is used.
	const hasKind = (kind: string) =>
		[...granted].some(
			(entry) => entry === kind || entry.startsWith(`${kind}:`),
		)

	const deny = (capability: string, permission: string) => () => {
		throw new Error(
			`Plugin "${pluginId}" cannot use ${capability}: the "${permission}" permission has not been granted.`,
		)
	}

	return Object.freeze({
		plugin: Object.freeze({
			id: pluginId,
			name: summary.manifest?.name ?? pluginId,
			version: summary.manifest?.version ?? '0.0.0',
		}),
		log: (...args: unknown[]) => console.log(`[plugin:${pluginId}]`, ...args),
		styles: Object.freeze(
			hasKind('style')
				? { add: (css: string) => addStyle(pluginId, css) }
				: { add: deny('styles', 'style') },
		),
		slots: Object.freeze(
			hasKind('slot')
				? {
						add: (slot: PluginSlotId, definition: SlotDefinition) =>
							addSlot(pluginId, slot, definition, granted),
					}
				: { add: deny('slots', 'slot') },
		),
		// Storage is also checked in Rust, which is the check that decides; this
		// one exists so an undeclared plugin gets the error immediately.
		storage: Object.freeze(
			hasKind('storage')
				? {
						get: (key: string) => pluginStorageGet(pluginId, key),
						set: (key: string, value: string) => pluginStorageSet(pluginId, key, value),
						remove: (key: string) => pluginStorageRemove(pluginId, key),
						keys: () => pluginStorageKeys(pluginId),
					}
				: {
						get: deny('storage', 'storage'),
						set: deny('storage', 'storage'),
						remove: deny('storage', 'storage'),
						keys: deny('storage', 'storage'),
					},
		),
	})
}

function addStyle(pluginId: string, css: string): void {
	const element = document.createElement('style')
	element.dataset.plugin = pluginId
	element.textContent = css
	// Appended rather than inserted: a theme's rules have to come after the
	// launcher's own stylesheets to stand a chance of winning the cascade.
	document.head.appendChild(element)
}

function addSlot(
	pluginId: string,
	slotId: PluginSlotId,
	definition: SlotDefinition,
	granted: Set<string>,
): void {
	if (!PLUGIN_SLOTS.includes(slotId)) {
		throw new Error(
			`Unknown slot "${slotId}". Available slots: ${PLUGIN_SLOTS.join(', ')}`,
		)
	}
	// Each slot is granted on its own: declaring `slot:sidebar.top` must not let
	// a plugin attach to every other slot as well.
	if (!granted.has(`slot:${slotId}`)) {
		throw new Error(
			`Plugin "${pluginId}" cannot use slot "${slotId}": the "slot:${slotId}" permission has not been granted.`,
		)
	}
	if (!definition || typeof definition.render !== 'function') {
		throw new Error('A slot needs a render() function.')
	}
	if (!definition.id) {
		throw new Error('A slot needs an id.')
	}

	const key = `${slotId}:${definition.id}`
	if (mounts.some((record) => record.pluginId === pluginId && record.key === key)) {
		throw new Error(`Slot "${key}" is already taken by this plugin.`)
	}

	let node: Node | null | void
	try {
		node = definition.render()
	} catch (error) {
		throw new Error(
			`Rendering slot "${key}" failed: ${describeError(error).message}`,
		)
	}
	if (!(node instanceof HTMLElement)) {
		throw new Error(`Slot "${key}" render() must return an element.`)
	}

	mounts.push({
		pluginId,
		key,
		slotId,
		element: node,
		container: null,
	})
	mountPending()
}

/**
 * Place every slot whose container now exists.
 *
 * Plugins start loading while the launcher is still rendering, so a container
 * is usually missing on the first pass. Watching the document is what makes a
 * plugin's UI appear once its slot is actually on screen.
 */
function mountPending(): void {
	for (const record of mounts) {
		if (record.container?.isConnected) {
			continue
		}
		const container = document.querySelector<HTMLElement>(
			`[data-plugin-slot="${record.slotId}"]`,
		)
		if (!container) {
			continue
		}
		container.appendChild(record.element)
		record.container = container
	}

	if (mounts.some((record) => !record.container?.isConnected) && !observer) {
		observer = new MutationObserver(() => mountPending())
		observer.observe(document.body, { childList: true, subtree: true })
	} else if (observer && mounts.every((record) => record.container?.isConnected)) {
		observer.disconnect()
		observer = null
	}
}

async function reportCrash(
	summary: PluginSummary,
	error: unknown,
	options: LoadPluginsOptions,
): Promise<void> {
	const { message, stack } = describeError(error)
	console.error(`[plugin:${summary.id}] crashed`, error)
	try {
		await pluginReportCrash(summary.id, message, stack)
	} catch (reportError) {
		// Reporting is best effort: the plugin is already being skipped, and a
		// failure here would only hide the original crash.
		console.error(`[plugin:${summary.id}] could not report crash`, reportError)
	}
	options.onCrash?.(summary, message)
}

function describeError(error: unknown): { message: string; stack?: string } {
	if (error instanceof Error) {
		return { message: error.message || error.name, stack: error.stack }
	}
	return { message: String(error) }
}

function joinPath(base: string, relative: string): string {
	const separator = base.includes('\\') ? '\\' : '/'
	return `${base.replace(/[\\/]+$/, '')}${separator}${relative.replace(/^[\\/]+/, '')}`
}

function cssEscape(value: string): string {
	return value.replace(/["\\]/g, '\\$&')
}
