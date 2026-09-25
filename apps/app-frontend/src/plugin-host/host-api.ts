/**
 * The curated surface a plugin may reach into the launcher through.
 *
 * Three registries, each an allowlist rather than a pass-through:
 *
 * - {@link HOST_API} — launcher functions a plugin may call, by name.
 * - {@link PLUGIN_EVENTS} — app events a plugin may subscribe to.
 * - {@link PLUGIN_REGIONS} — DOM regions a plugin may restructure.
 *
 * These are allowlists on purpose. The frontend can already invoke every IPC
 * command, so a plugin sharing the page could reach them regardless (see the
 * L1-b decision); naming a small set here is about intent and stability, not a
 * security boundary. The boundary that decides is still in Rust.
 */

import { get_default_user, users } from '@/helpers/auth'
import { get as getInstance, list as listInstances } from '@/helpers/instance'
import { library_list } from '@/helpers/library'

/** A launcher function exposed to plugins under a stable name. */
export type HostApiFn = (...args: unknown[]) => Promise<unknown>

/**
 * Read-only to start with. Every entry is a promise-returning function so the
 * plugin side is uniform, and nothing here mutates launcher state — a plugin
 * that needs to change something does it through a dedicated, separately-gated
 * capability, not through this general call surface.
 */
export const HOST_API: Record<string, HostApiFn> = {
	'instance.list': (libraryPath?: unknown) =>
		listInstances(
			typeof libraryPath === 'string' ? libraryPath : undefined,
		),
	'instance.get': (instanceId: unknown) => {
		if (typeof instanceId !== 'string') {
			return Promise.reject(new Error('instance.get needs an instance id'))
		}
		return getInstance(instanceId)
	},
	'library.list': () => library_list(),
	// The active Minecraft account's username, resolved from the default-user
	// UUID and the account list. Just the name — not the full credentials.
	'auth.default_username': async () => {
		const uuid = await get_default_user()
		if (!uuid) return null
		const list = await users()
		if (!Array.isArray(list)) return null
		const match = list.find(
			(user) => (user as { profile?: { id?: unknown } })?.profile?.id === uuid,
		) as { profile?: { name?: unknown } } | undefined
		const name = match?.profile?.name
		return typeof name === 'string' ? name : null
	},
}

export const HOST_API_NAMES = Object.freeze(Object.keys(HOST_API))

/**
 * App events a plugin may subscribe to.
 *
 * Deliberately narrower than the full event set: `friend` is other people's
 * data, and `notification` / `loading` / `command` / `ads_consent_required` /
 * `onboarding_checklist` are launcher-internal UI signals a plugin has no
 * business reacting to. What remains is the lifecycle of instances, processes
 * and the library — the things a plugin legitimately extends.
 */
export const PLUGIN_EVENTS = Object.freeze([
	'instance',
	'instance_groups_changed',
	'instance_bulk_update_progress',
	'process',
	'install_job',
	'library_changed',
	'log',
] as const)

export type PluginEventType = (typeof PLUGIN_EVENTS)[number]

export function isPluginEvent(type: string): type is PluginEventType {
	return (PLUGIN_EVENTS as readonly string[]).includes(type)
}

/**
 * DOM regions a plugin with the matching `region:<name>` permission may take
 * over. Each maps to the `data-plugin-region` marker the launcher renders.
 *
 * High risk by nature: handing over a container lets a plugin restructure that
 * part of the chrome, and a plugin that breaks it breaks that region. Kept to
 * the three structural areas a theme would legitimately reshape.
 */
export const PLUGIN_REGIONS = Object.freeze([
	'navbar',
	'topbar',
	'sidebar',
] as const)

export type PluginRegionId = (typeof PLUGIN_REGIONS)[number]

export function isPluginRegion(name: string): name is PluginRegionId {
	return (PLUGIN_REGIONS as readonly string[]).includes(name)
}
