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
	computed,
	defineComponent,
	h,
	onMounted,
	onUnmounted,
	reactive,
	ref,
	render,
	shallowRef,
	watch,
	type Component,
} from 'vue'

import router from '@/routes'

import {
	HOST_API,
	isPluginEvent,
	isPluginRegion,
	type PluginEventType,
	type PluginRegionId,
} from './host-api'
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
 * The launcher's app-event bus, handed in by App.vue at load time. The loader
 * runs outside any component, so it cannot inject the bus the way a component
 * would — it has to be given.
 */
export interface PluginEventBus {
	on(type: string, handler: (payload: unknown) => void): () => void
}

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
	/**
	 * A Vue component, rendered with the launcher's own Vue. Preferred over
	 * `render` — a component gets reactivity, and the plugin never has to build
	 * DOM by hand.
	 */
	component?: unknown
	props?: Record<string, unknown>
	/** Raw DOM, for the rare case where a component is the wrong tool. */
	render?: () => Node | null | void
}

export interface PluginRouteDefinition {
	/** Absolute, and must not collide with a route the launcher already owns. */
	path: string
	name?: string
	component: unknown
}

/** The slice of Vue a plugin is given. */
export interface PluginVueRuntime {
	readonly h: typeof h
	readonly ref: typeof ref
	readonly reactive: typeof reactive
	readonly computed: typeof computed
	readonly watch: typeof watch
	readonly shallowRef: typeof shallowRef
	readonly defineComponent: typeof defineComponent
	readonly onMounted: typeof onMounted
	readonly onUnmounted: typeof onUnmounted
}

export interface PluginHostApi {
	readonly plugin: {
		readonly id: string
		readonly name: string
		readonly version: string
	}
	readonly vue: PluginVueRuntime
	readonly styles: {
		add(css: string): void
	}
	readonly slots: {
		add(slot: PluginSlotId, definition: SlotDefinition): void
	}
	readonly routes: {
		add(route: PluginRouteDefinition): void
	}
	/** Navigation is not gated: it moves the user, it cannot act on their behalf. */
	readonly router: {
		push(to: string): void
		replace(to: string): void
		current(): string
	}
	readonly storage: {
		get(key: string): Promise<string | null>
		set(key: string, value: string): Promise<void>
		remove(key: string): Promise<void>
		keys(): Promise<string[]>
	}
	readonly events: {
		/** Returns an unsubscribe function. Auto-cleaned when the plugin unloads. */
		on(type: PluginEventType, handler: (payload: unknown) => void): () => void
	}
	readonly hostApi: {
		call(name: string, ...args: unknown[]): Promise<unknown>
	}
	readonly regions: {
		/** The container element for a region, to restructure in place. */
		get(name: PluginRegionId): HTMLElement
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
	/** Tears down a Vue-rendered slot; absent on the raw-DOM path. */
	unmount?: () => void
}

export interface LoadPluginsOptions {
	/** Called after a plugin threw and the launcher switched it off. */
	onCrash?: (summary: PluginSummary, message: string) => void
	/** The launcher's app-event bus, so plugins can subscribe to app events. */
	events?: PluginEventBus
}

const instances = new Map<string, PluginSummary>()
const mounts: MountRecord[] = []
const pluginRoutes: { pluginId: string; name: string }[] = []
const eventSubscriptions: { pluginId: string; off: () => void }[] = []
let observer: MutationObserver | null = null
let eventBus: PluginEventBus | null = null

export async function loadPlugins(options: LoadPluginsOptions = {}): Promise<void> {
	eventBus = options.events ?? null
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
		const record = mounts[index]
		if (record.pluginId !== pluginId) {
			continue
		}
		record.unmount?.()
		record.element.remove()
		mounts.splice(index, 1)
	}
	for (let index = pluginRoutes.length - 1; index >= 0; index -= 1) {
		if (pluginRoutes[index].pluginId === pluginId) {
			router.removeRoute(pluginRoutes[index].name)
			pluginRoutes.splice(index, 1)
		}
	}
	for (let index = eventSubscriptions.length - 1; index >= 0; index -= 1) {
		if (eventSubscriptions[index].pluginId === pluginId) {
			eventSubscriptions[index].off()
			eventSubscriptions.splice(index, 1)
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

/**
 * The launcher's own Vue, handed to plugins so a component they write shares
 * the app's reactivity. A plugin that bundled its own copy of Vue would produce
 * vnodes from a different instance, which the launcher's renderer cannot mount.
 */
const VUE_RUNTIME: PluginVueRuntime = Object.freeze({
	h,
	ref,
	reactive,
	computed,
	watch,
	shallowRef,
	defineComponent,
	onMounted,
	onUnmounted,
})

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
		vue: VUE_RUNTIME,
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
		routes: Object.freeze(
			hasKind('route')
				? {
						add: (route: PluginRouteDefinition) =>
							addRoute(pluginId, route),
					}
				: { add: deny('routes', 'route') },
		),
		router: Object.freeze({
			push: (to: string) => {
				void router.push(to)
			},
			replace: (to: string) => {
				void router.replace(to)
			},
			current: () => router.currentRoute.value.fullPath,
		}),
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
		events: Object.freeze(
			hasKind('event')
				? {
						on: (type: PluginEventType, handler: (payload: unknown) => void) =>
							subscribeEvent(pluginId, granted, type, handler),
					}
				: { on: deny('events', 'event') },
		),
		hostApi: Object.freeze(
			hasKind('hostapi')
				? {
						call: (name: string, ...args: unknown[]) =>
							callHostApi(granted, name, args),
					}
				: { call: deny('hostApi', 'hostapi') },
		),
		regions: Object.freeze(
			hasKind('region')
				? { get: (name: PluginRegionId) => getRegion(granted, name) }
				: { get: deny('regions', 'region') },
		),
	})
}

/**
 * Subscribe a plugin to one app event.
 *
 * Gated twice: the event has to be on the allowlist (a plugin cannot listen to
 * launcher-internal signals), and the plugin has to hold `event:<type>` for
 * that specific type. The subscription is tracked so `unloadPlugin` can drop it
 * — a plugin that unloads must stop receiving events.
 */
function subscribeEvent(
	pluginId: string,
	granted: Set<string>,
	type: PluginEventType,
	handler: (payload: unknown) => void,
): () => void {
	if (!isPluginEvent(type)) {
		throw new Error(`Event "${type}" is not available to plugins.`)
	}
	if (!granted.has(`event:${type}`)) {
		throw new Error(
			`Plugin "${pluginId}" cannot listen to "${type}": the "event:${type}" permission has not been granted.`,
		)
	}
	if (!eventBus) {
		throw new Error('The event bus is not available.')
	}
	if (typeof handler !== 'function') {
		throw new Error('An event subscription needs a handler function.')
	}

	// Wrapped so one plugin's throwing handler cannot take down the emit loop
	// that dispatches to the launcher and every other plugin.
	const off = eventBus.on(type, (payload) => {
		try {
			handler(payload)
		} catch (error) {
			console.error(`[plugin:${pluginId}] "${type}" handler threw`, error)
		}
	})
	const record = { pluginId, off }
	eventSubscriptions.push(record)
	return () => {
		off()
		const index = eventSubscriptions.indexOf(record)
		if (index !== -1) {
			eventSubscriptions.splice(index, 1)
		}
	}
}

/** Call a launcher function by name, if the plugin holds `hostapi:<name>`. */
function callHostApi(
	granted: Set<string>,
	name: string,
	args: unknown[],
): Promise<unknown> {
	if (!granted.has(`hostapi:${name}`)) {
		throw new Error(
			`This plugin cannot call "${name}": the "hostapi:${name}" permission has not been granted.`,
		)
	}
	const fn = HOST_API[name]
	if (!fn) {
		throw new Error(`Unknown host API "${name}".`)
	}
	return fn(...args)
}

/** The container element for a region, if the plugin holds `region:<name>`. */
function getRegion(granted: Set<string>, name: PluginRegionId): HTMLElement {
	if (!isPluginRegion(name)) {
		throw new Error(`Unknown region "${name}".`)
	}
	if (!granted.has(`region:${name}`)) {
		throw new Error(
			`This plugin cannot access region "${name}": the "region:${name}" permission has not been granted.`,
		)
	}
	const element = document.querySelector<HTMLElement>(
		`[data-plugin-region="${name}"]`,
	)
	if (!element) {
		throw new Error(`Region "${name}" is not on screen.`)
	}
	return element
}

function addStyle(pluginId: string, css: string): void {
	const element = document.createElement('style')
	element.dataset.plugin = pluginId
	element.textContent = css
	// Appended rather than inserted: a theme's rules have to come after the
	// launcher's own stylesheets to stand a chance of winning the cascade.
	document.head.appendChild(element)
}

/**
 * Give a plugin a page of its own.
 *
 * The route is named after the plugin rather than trusting a caller-supplied
 * name, because the name is what `unloadPlugin` removes it by — a plugin able
 * to pick any name could delete a launcher route instead of its own.
 */
function addRoute(pluginId: string, route: PluginRouteDefinition): void {
	if (
		!route ||
		typeof route.path !== 'string' ||
		!route.path.startsWith('/')
	) {
		throw new Error(
			'A route needs an absolute path starting with "/".',
		)
	}
	if (route.component === undefined || route.component === null) {
		throw new Error(`Route "${route.path}" needs a component.`)
	}
	if (router.resolve(route.path).matched.length > 0) {
		throw new Error(`Route path "${route.path}" is already taken.`)
	}

	const name = `plugin:${pluginId}:${route.path}`
	router.addRoute({
		path: route.path,
		name,
		component: route.component as Component,
	})
	pluginRoutes.push({ pluginId, name })
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
	if (!definition || !definition.id) {
		throw new Error('A slot needs an id.')
	}
	const hasComponent =
		definition.component !== undefined && definition.component !== null
	if (!hasComponent && typeof definition.render !== 'function') {
		throw new Error(
			'A slot needs either a component or a render() function.',
		)
	}

	const key = `${slotId}:${definition.id}`
	if (mounts.some((record) => record.pluginId === pluginId && record.key === key)) {
		throw new Error(`Slot "${key}" is already taken by this plugin.`)
	}

	let element: HTMLElement
	let unmount: (() => void) | undefined

	if (hasComponent) {
		element = document.createElement('div')
		render(
			h(
				definition.component as Component,
				definition.props ?? {},
			),
			element,
		)
		unmount = () => render(null, element)
	} else {
		let node: Node | null | void
		try {
			node = definition.render?.()
		} catch (error) {
			throw new Error(
				`Rendering slot "${key}" failed: ${describeError(error).message}`,
			)
		}
		if (!(node instanceof HTMLElement)) {
			throw new Error(`Slot "${key}" render() must return an element.`)
		}
		element = node
	}

	mounts.push({
		pluginId,
		key,
		slotId,
		element,
		container: null,
		unmount,
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
