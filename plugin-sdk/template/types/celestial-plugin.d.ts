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

		/** Log with a `[plugin:<id>]` prefix. */
		log(...args: unknown[]): void
	}

	/** Implement this in your entry module. */
	export function activate(api: PluginHostApi): void | Promise<void>
}
