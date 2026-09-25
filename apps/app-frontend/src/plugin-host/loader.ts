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
	Teleport,
	watch,
	type Component,
	type ComputedRef,
} from 'vue'
import * as VueRuntime from 'vue'

import { arch, platform } from '@tauri-apps/plugin-os'
import { Button, DropdownSelect, Input, Toggle } from '@modrinth/ui'

import router from '@/routes'

/**
 * The global a plugin bundle's `vue` imports resolve to.
 *
 * A plugin is a standalone module loaded from a blob/asset URL, so a bare
 * `import ... from 'vue'` in it has nothing to resolve against at runtime, and
 * bundling its own Vue would give it a different instance the launcher cannot
 * render. The plugin SDK's build rewrites `vue` to read this global, so a
 * plugin — including one compiled from `.vue` single-file components — shares
 * the launcher's exact Vue.
 */
const VUE_GLOBAL_KEY = '__CELESTIAL_PLUGIN_VUE__'

function exposeVueForPlugins(): void {
	;(globalThis as Record<string, unknown>)[VUE_GLOBAL_KEY] ??= VueRuntime
}

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
	pluginLanAnnounce,
	pluginLanCleanup,
	pluginLanStop,
	pluginNetworkFetch,
	pluginReadEntry,
	pluginReportCrash,
	pluginSettingsGet,
	pluginSidecarCleanup,
	pluginSidecarEnsure,
	pluginSidecarRequest,
	pluginSidecarStart,
	pluginSidecarStatus,
	pluginSidecarStop,
	pluginStorageGet,
	pluginStorageKeys,
	pluginStorageRemove,
	pluginStorageSet,
	type SidecarRequestInit,
	type SidecarResponse,
	type SidecarStarted,
	type SidecarStatus,
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
	'sidebar.after-jumpback',
	'sidebar.after-account',
	'sidebar.bottom',
	'home.top',
	'home.middle',
	'home.bottom',
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
	/** Render a node elsewhere in the document (e.g. a modal onto `<body>`). */
	readonly Teleport: typeof Teleport
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
		/** The current path, as a snapshot. */
		current(): string
		/** The current path, reactive — for highlighting an active nav button. */
		readonly currentPath: ComputedRef<string>
	}
	readonly storage: {
		get(key: string): Promise<string | null>
		set(key: string, value: string): Promise<void>
		remove(key: string): Promise<void>
		keys(): Promise<string[]>
	}
	/**
	 * Values of the plugin's declared settings (from its manifest), which the
	 * user configures on the plugin page. Read-only here — writing is the user's
	 * action. Ungated; declaring a setting is enough.
	 */
	readonly settings: {
		get(key: string): Promise<string | null>
		all(): Promise<Record<string, string>>
		/**
		 * Render custom UI into this plugin's settings modal, below the
		 * declared fields — for anything the declarative settings cannot
		 * express. Ungated: it is the plugin's own settings surface.
		 */
		render(definition: SlotDefinition): void
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
	readonly net: {
		/** Fetch through the launcher. Requires `network:<host>` for the URL. */
		fetch(url: string, options?: PluginFetchOptions): Promise<PluginFetchResult>
	}
	/**
	 * Download and run a native sidecar binary, then talk to it over localhost.
	 * The most powerful capability a plugin can hold — it runs native code.
	 * Requires `sidecar` (and `network:<host>` for the download host).
	 */
	readonly sidecar: {
		ensure(
			key: string,
			url: string,
			options?: { sha512?: string; sha512Url?: string; archive?: string; version?: string },
		): Promise<void>
		start(key: string, args: string[], portFile?: boolean): Promise<SidecarStarted>
		request(handle: number, request: SidecarRequestInit): Promise<SidecarResponse>
		stop(handle: number): Promise<void>
		status(key: string): Promise<SidecarStatus>
	}
	/** Announce a Minecraft LAN world on the local network. Requires `lan`. */
	readonly lan: {
		announce(motd: string, port: number): Promise<{ handle: number }>
		stop(handle: number): Promise<void>
	}
	/** The host OS and architecture. Ungated. */
	readonly platform: {
		readonly os: string
		readonly arch: string
	}
	/**
	 * The launcher's own UI components, so a plugin's chrome matches the rest of
	 * the app rather than re-implementing it. Ungated — these only render, they
	 * grant no capability. (These four read no injected context, so they work in
	 * a plugin's own mount tree; components that need launcher-wide context, like
	 * `NewModal`, are not exposed.)
	 */
	readonly ui: {
		readonly Button: Component
		readonly Input: Component
		readonly Toggle: Component
		readonly DropdownSelect: Component
	}
	log(...args: unknown[]): void
}

export interface PluginFetchOptions {
	method?: string
	headers?: Record<string, string>
	body?: string
}

export interface PluginFetchResult {
	status: number
	ok: boolean
	headers: Record<string, string>
	body: string
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
	/** A chrome slot id, or a per-plugin settings-modal container id. */
	slotId: PluginSlotId | string
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
	exposeVueForPlugins()
	// Only overwrite when one was provided: this is called again to apply a
	// plugin being enabled or disabled, and clearing the bus would silently stop
	// every existing subscription from receiving events.
	if (options.events) {
		eventBus = options.events
	}
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
	// Kill any native sidecars and LAN announcers this plugin started; a plugin
	// that is unloaded must not leave a native process or a multicast beacon
	// running. Fire-and-forget: unloadPlugin is synchronous.
	void pluginSidecarCleanup(pluginId).catch(() => {})
	void pluginLanCleanup(pluginId).catch(() => {})
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
	const entryPath = joinPath(summary.path, entry)
	const url = pluginAssetUrl(entryPath)
	try {
		const module = (await import(/* @vite-ignore */ url)) as PluginModule
		return module.activate ?? module.default?.activate
	} catch (assetError) {
		let source: string | null = null
		try {
			source = await pluginReadEntry(summary.id)
		} catch {
			source = null
		}
		if (source === null) {
			// The usual cause is an `entry` that does not match where the file
			// actually is — most often a manifest copied alongside a bundle that
			// was flattened out of `dist/`. Say the path rather than leaving the
			// reader to guess from an opaque module-load failure.
			throw new Error(
				`The plugin's entry file could not be read at "${entryPath}". ` +
					`Check that "entry" in manifest.json ("${entry}") points at a file inside the plugin folder. ` +
					`(the module load itself failed with: ${describeError(assetError).message})`,
			)
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
	Teleport,
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
			currentPath: computed(() => router.currentRoute.value.fullPath),
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
		// Ungated: declared settings are user-owned config, not a plugin
		// capability. Reads go through the same per-plugin file as storage.
		settings: Object.freeze({
			get: async (key: string) => {
				const all = await pluginSettingsGet(pluginId)
				return key in all ? all[key] : null
			},
			all: () => pluginSettingsGet(pluginId),
			render: (definition: SlotDefinition) =>
				addSlot(
					pluginId,
					`plugin-settings:${pluginId}`,
					definition,
					granted,
					{ skipGrantCheck: true },
				),
		}),
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
		net: Object.freeze(
			hasKind('network')
				? {
						fetch: (url: string, opts?: PluginFetchOptions) =>
							pluginFetch(pluginId, url, opts),
					}
				: { fetch: deny('net', 'network') },
		),
		sidecar: Object.freeze(
			hasKind('sidecar')
				? {
						ensure: (
							key: string,
							url: string,
							options?: {
								sha512?: string
								sha512Url?: string
								archive?: string
								version?: string
							},
						) => pluginSidecarEnsure(pluginId, key, url, options),
						start: (key: string, args: string[], portFile?: boolean) =>
							pluginSidecarStart(pluginId, key, args, portFile ?? false),
						request: (handle: number, request: SidecarRequestInit) =>
							pluginSidecarRequest(pluginId, handle, request),
						stop: (handle: number) => pluginSidecarStop(pluginId, handle),
						status: (key: string) => pluginSidecarStatus(pluginId, key),
					}
				: {
						ensure: deny('sidecar', 'sidecar'),
						start: deny('sidecar', 'sidecar'),
						request: deny('sidecar', 'sidecar'),
						stop: deny('sidecar', 'sidecar'),
						status: deny('sidecar', 'sidecar'),
					},
		),
		lan: Object.freeze(
			hasKind('lan')
				? {
						announce: (motd: string, port: number) =>
							pluginLanAnnounce(pluginId, motd, port),
						stop: (handle: number) => pluginLanStop(pluginId, handle),
					}
				: {
						announce: deny('lan', 'lan'),
						stop: deny('lan', 'lan'),
					},
		),
		platform: Object.freeze({
			os: platform(),
			arch: arch(),
		}),
		ui: Object.freeze({
			Button,
			Input,
			Toggle,
			DropdownSelect,
		}),
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

/**
 * Fetch through the launcher's Rust side.
 *
 * The host is not checked here — the whole point is that the webview cannot
 * reach the network, so the real check is in Rust against the plugin's
 * `network:<host>` grant. This wrapper only shapes the call.
 */
function pluginFetch(
	pluginId: string,
	url: string,
	options?: PluginFetchOptions,
): Promise<PluginFetchResult> {
	if (typeof url !== 'string' || !url) {
		throw new Error('fetch needs a URL.')
	}
	return pluginNetworkFetch(pluginId, {
		url,
		method: options?.method,
		headers: options?.headers,
		body: options?.body,
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

	const name = `plugin:${pluginId}:${route.path}`

	// Drop a route this same plugin registered under the same name on an earlier
	// activation that failed partway through. Without this, one throw after
	// `routes.add` would leave the route registered and block every retry for
	// the rest of the session.
	for (let index = pluginRoutes.length - 1; index >= 0; index -= 1) {
		if (pluginRoutes[index].name === name) {
			router.removeRoute(name)
			pluginRoutes.splice(index, 1)
		}
	}

	// Compare by exact path against every existing record rather than
	// `router.resolve(...).matched`, which also reports a match for the
	// launcher's parametrised routes (`/:projectType(...)/:id/...`) and would
	// reject a plugin path like `/plugin/terracotta` with no real clash.
	if (router.getRoutes().some((record) => record.path === route.path)) {
		throw new Error(
			`Route path "${route.path}" is already taken. Launcher routes use ` +
				`reserved prefixes like "/plugin/:id" (Modrinth project pages) — ` +
				`register yours under a distinct path such as "/plugins/<your-id>".`,
		)
	}

	router.addRoute({
		path: route.path,
		name,
		component: route.component as Component,
	})
	pluginRoutes.push({ pluginId, name })
}

function addSlot(
	pluginId: string,
	slotId: PluginSlotId | string,
	definition: SlotDefinition,
	granted: Set<string>,
	options: { skipGrantCheck?: boolean } = {},
): void {
	// The settings modal renders a per-plugin container (`plugin-settings:<id>`)
	// that is not a named chrome slot and needs no `slot:` grant — it is the
	// plugin's own settings surface. Every other slot must be known and granted.
	if (!options.skipGrantCheck) {
		if (!PLUGIN_SLOTS.includes(slotId as PluginSlotId)) {
			throw new Error(
				`Unknown slot "${slotId}". Available slots: ${PLUGIN_SLOTS.join(', ')}`,
			)
		}
		// Each slot is granted on its own: declaring `slot:sidebar.top` must not
		// let a plugin attach to every other slot as well.
		if (!granted.has(`slot:${slotId}`)) {
			throw new Error(
				`Plugin "${pluginId}" cannot use slot "${slotId}": the "slot:${slotId}" permission has not been granted.`,
			)
		}
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
 *
 * The observer stays live as long as any plugin has a slot mounted, not just
 * until the first placement: page-level slots (`home.*`) live on a KeepAlive
 * page that unmounts on navigation and remounts on return, so a placed element
 * can be detached and a fresh container can reappear at any time.
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

	if (mounts.length > 0 && !observer) {
		observer = new MutationObserver(() => mountPending())
		observer.observe(document.body, { childList: true, subtree: true })
	} else if (observer && mounts.length === 0) {
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
	if (typeof error === 'string') {
		return { message: error }
	}
	if (error instanceof Event) {
		// A blocked or failed module load surfaces as an Event in some cases,
		// and `String(event)` is "[object Event]".
		return { message: `module load failed (${error.type} event)` }
	}
	if (error && typeof error === 'object') {
		// A failed dynamic import rejects with a plain object rather than an
		// Error in some webviews. `String(object)` renders as "[object Object]",
		// which tells whoever reads crash.log nothing at all — so look for the
		// usual fields, then fall back to serialising the whole thing.
		const record = error as Record<string, unknown>
		const stack = typeof record.stack === 'string' ? record.stack : undefined
		for (const key of ['message', 'error', 'reason', 'detail']) {
			const value = record[key]
			if (typeof value === 'string' && value) {
				return { message: value, stack }
			}
		}
		try {
			const serialised = JSON.stringify(error)
			if (serialised && serialised !== '{}') {
				return { message: serialised, stack }
			}
		} catch {
			// Circular or otherwise unserialisable; fall through.
		}
		return { message: `unreadable rejection: ${String(error)}`, stack }
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
