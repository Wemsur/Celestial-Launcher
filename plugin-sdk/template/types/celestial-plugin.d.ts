/**
 * Type definitions for a Celestial Launcher plugin.
 *
 * A plugin's entry module exports `activate(api)`. Everything it can do is on
 * that `api` object — the launcher never lets a plugin reach further, and a
 * capability the plugin did not declare in its manifest throws when used rather
 * than being silently missing.
 *
 * These types mirror the host at plugin API version 1. Keep `api_version` in
 * the manifest in step with the version documented here.
 */

declare module '@celestial/plugin' {
	import type {
		Component,
		ComputedRef,
		Ref,
		ShallowRef,
		WatchSource,
		WatchCallback,
		WatchOptions,
		WatchStopHandle,
	} from 'vue'

	/** A named place in the launcher's chrome a plugin may attach UI to. */
	export type PluginSlotId =
		| 'topbar.left'
		| 'topbar.center'
		| 'topbar.right'
		| 'navbar.bottom'
		| 'sidebar.top'
		| 'sidebar.bottom'
		| 'home.top'
		| 'home.middle'
		| 'home.bottom'

	/** An app event a plugin may subscribe to. */
	export type PluginEventType =
		| 'instance'
		| 'instance_groups_changed'
		| 'instance_bulk_update_progress'
		| 'process'
		| 'install_job'
		| 'library_changed'
		| 'log'

	/** A structural region a plugin may take over, given `region:<name>`. */
	export type PluginRegionId = 'navbar' | 'topbar' | 'sidebar'

	export interface SlotDefinition {
		/** Unique within the plugin, per slot. */
		id: string
		/** A Vue component (preferred). Rendered with the launcher's Vue. */
		component?: Component
		props?: Record<string, unknown>
		/** Raw DOM, for when a component is the wrong tool. */
		render?: () => Node | null | void
	}

	export interface RouteDefinition {
		/** Absolute path; must not collide with a route the launcher owns. */
		path: string
		name?: string
		component: Component
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

	export interface SidecarStarted {
		handle: number
		/** The port the sidecar reported via its port file, if one was requested. */
		port: number | null
	}

	export interface SidecarRequestInit {
		/** Path plus query string, sent verbatim (so repeated query keys work). */
		path: string
		method?: string
		headers?: Record<string, string>
		body?: string
	}

	export interface SidecarResponse {
		status: number
		ok: boolean
		body: string
	}

	export interface SidecarStatus {
		installed: boolean
		/** The version recorded when the sidecar was installed, if any. */
		version: string | null
	}

	/** A setting declared in the manifest, rendered by the launcher. */
	export interface SettingsDefinition {
		id: string
		component?: Component
		props?: Record<string, unknown>
		render?: () => Node | null | void
	}

	/**
	 * The subset of Vue handed to plugins. Prefer these over importing from
	 * `vue` directly if you are writing plain render functions; either resolves
	 * to the launcher's own Vue.
	 */
	export interface PluginVueRuntime {
		h: typeof import('vue').h
		ref<T>(value: T): Ref<T>
		reactive: typeof import('vue').reactive
		computed<T>(getter: () => T): ComputedRef<T>
		watch<T>(
			source: WatchSource<T>,
			cb: WatchCallback<T>,
			options?: WatchOptions,
		): WatchStopHandle
		shallowRef<T>(value: T): ShallowRef<T>
		defineComponent: typeof import('vue').defineComponent
		onMounted(hook: () => void): void
		onUnmounted(hook: () => void): void
	}

	export interface PluginHostApi {
		/** This plugin's own identity, from its manifest. */
		readonly plugin: {
			readonly id: string
			readonly name: string
			readonly version: string
		}

		/** The launcher's Vue. Components you build with it mount in the host. */
		readonly vue: PluginVueRuntime

		/** Inject CSS. Requires the `style` permission. */
		readonly styles: {
			add(css: string): void
		}

		/** Attach UI to a slot. Requires `slot:<id>` for that slot. */
		readonly slots: {
			add(slot: PluginSlotId, definition: SlotDefinition): void
		}

		/** Register a page of your own. Requires the `route` permission. */
		readonly routes: {
			add(route: RouteDefinition): void
		}

		/** Navigate the launcher. Not gated — it only moves the user. */
		readonly router: {
			push(to: string): void
			replace(to: string): void
			current(): string
		}

		/** Your own key/value storage. Requires the `storage` permission. */
		readonly storage: {
			get(key: string): Promise<string | null>
			set(key: string, value: string): Promise<void>
			remove(key: string): Promise<void>
			keys(): Promise<string[]>
		}

		/**
		 * Values of the settings declared in your manifest, configured by the
		 * user on the plugin page. Read-only here; ungated. `render` adds custom
		 * UI to the bottom of your settings modal.
		 */
		readonly settings: {
			get(key: string): Promise<string | null>
			all(): Promise<Record<string, string>>
			render(definition: SettingsDefinition): void
		}

		/** Subscribe to an app event. Requires `event:<type>` for that event. */
		readonly events: {
			/** Returns an unsubscribe function; also cleaned up on unload. */
			on(type: PluginEventType, handler: (payload: unknown) => void): () => void
		}

		/** Call a launcher function by name. Requires `hostapi:<name>`. */
		readonly hostApi: {
			call(name: 'instance.list', libraryPath?: string): Promise<unknown[]>
			call(name: 'instance.get', instanceId: string): Promise<unknown>
			call(name: 'library.list'): Promise<unknown>
			call(name: string, ...args: unknown[]): Promise<unknown>
		}

		/** Get a structural region's container. Requires `region:<name>`. */
		readonly regions: {
			get(name: PluginRegionId): HTMLElement
		}

		/** Fetch through the launcher. Requires `network:<host>` for the URL. */
		readonly net: {
			fetch(url: string, options?: PluginFetchOptions): Promise<PluginFetchResult>
		}

		/**
		 * Download and run a native sidecar binary, then talk to it over
		 * localhost. The most powerful thing a plugin can do — native code
		 * outside the webview. Requires `sidecar`, plus `network:<host>` for the
		 * download host. Sidecars are killed when the plugin unloads and on exit.
		 */
		readonly sidecar: {
			/**
			 * Ensure a sidecar is downloaded, verified, and unpacked under `key`.
			 * `archive` is `'tar.gz'` (default), `'zip'`, or `'none'`. Idempotent.
			 */
			ensure(
				key: string,
				url: string,
				options?: {
					sha512?: string
					sha512Url?: string
					archive?: string
					version?: string
				},
			): Promise<void>
			/**
			 * Spawn the sidecar. With `portFile`, the launcher rewrites the
			 * `{{PORT_FILE}}` token in `args` to a file it then polls for the
			 * `{"port":N}` the sidecar writes, returning that port.
			 */
			start(key: string, args: string[], portFile?: boolean): Promise<SidecarStarted>
			/** Send a control request to the running sidecar over 127.0.0.1. */
			request(handle: number, request: SidecarRequestInit): Promise<SidecarResponse>
			stop(handle: number): Promise<void>
			/** Whether the sidecar is installed, and its recorded version. */
			status(key: string): Promise<SidecarStatus>
		}

		/**
		 * Announce a Minecraft "open to LAN" world on the local network so the
		 * user's own Minecraft client discovers it. Requires `lan`.
		 */
		readonly lan: {
			announce(motd: string, port: number): Promise<{ handle: number }>
			stop(handle: number): Promise<void>
		}

		/** The host OS and architecture, e.g. `{ os: 'windows', arch: 'x86_64' }`. Ungated. */
		readonly platform: {
			readonly os: string
			readonly arch: string
		}

		/** Log with a `[plugin:<id>]` prefix. */
		log(...args: unknown[]): void
	}

	/** Implement this in your entry module. */
	export function activate(api: PluginHostApi): void | Promise<void>
}
