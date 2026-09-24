/**
 * The plugin store: a `plugins.json` index hosted on GitHub, fetched and
 * rendered as an install list.
 *
 * The index is third-party data (anyone can open a PR to it), so it is treated
 * as such: fetched read-only, every entry validated, and nothing is executed —
 * a store entry only points at a download. Installing still goes through the
 * same manifest validation and permission approval as a hand-placed plugin.
 */

const DEFAULT_REGISTRY_URL =
	'https://raw.githubusercontent.com/Wemsur/Celestial-Plugin-Store/refs/heads/main/plugins.json'

const REGISTRY_URL_KEY = 'celestial-plugin-store-url'

export interface StorePlugin {
	id: string
	name: string
	description?: string
	author?: string
	version?: string
	/** The plugin's own page/repo, shown as a link. */
	repo?: string
	/** Direct download of the packaged plugin (a .zip). */
	download: string
	icon?: string
	tags?: string[]
	apiVersion?: number
}

interface RegistryFile {
	version?: number
	plugins?: unknown[]
}

/** Where the store index is fetched from. Overridable so you can point at your
 *  own fork or a staging index without a rebuild. */
export function registryUrl(): string {
	if (typeof localStorage === 'undefined') {
		return DEFAULT_REGISTRY_URL
	}
	return localStorage.getItem(REGISTRY_URL_KEY) || DEFAULT_REGISTRY_URL
}

export function setRegistryUrl(url: string): void {
	if (typeof localStorage === 'undefined') return
	if (url.trim()) {
		localStorage.setItem(REGISTRY_URL_KEY, url.trim())
	} else {
		localStorage.removeItem(REGISTRY_URL_KEY)
	}
}

/**
 * Fetch and validate the store index.
 *
 * Uses the browser `fetch` rather than the plugin network proxy on purpose:
 * this is the launcher fetching its own configured index, not a plugin reaching
 * out, so it is not subject to plugin grants. The registry host must be allowed
 * by the launcher CSP's `connect-src`.
 */
export async function fetchStorePlugins(): Promise<StorePlugin[]> {
	const response = await fetch(registryUrl(), {
		headers: { accept: 'application/json' },
	})
	if (!response.ok) {
		throw new Error(`插件商店请求失败：HTTP ${response.status}`)
	}

	const data = (await response.json()) as RegistryFile
	if (!data || !Array.isArray(data.plugins)) {
		throw new Error('插件商店索引格式无效：缺少 plugins 数组')
	}

	return data.plugins
		.map(normalizeEntry)
		.filter((entry): entry is StorePlugin => entry !== null)
}

/** Coerce and vet one raw registry entry; drops anything unusable. */
function normalizeEntry(raw: unknown): StorePlugin | null {
	if (!raw || typeof raw !== 'object') return null
	const record = raw as Record<string, unknown>

	const id = typeof record.id === 'string' ? record.id : null
	const name = typeof record.name === 'string' ? record.name : null
	const download = typeof record.download === 'string' ? record.download : null
	// An entry with no id, name, or download link cannot be shown or installed.
	if (!id || !name || !download) return null

	// Only http(s) downloads: the installer will fetch this, and a `file:` or
	// other scheme in third-party data has no legitimate use here.
	if (!/^https?:\/\//i.test(download)) return null

	const str = (value: unknown) =>
		typeof value === 'string' ? value : undefined

	return {
		id,
		name,
		download,
		description: str(record.description),
		author: str(record.author),
		version: str(record.version),
		repo: str(record.repo),
		icon: str(record.icon),
		apiVersion:
			typeof record.apiVersion === 'number' ? record.apiVersion : undefined,
		tags: Array.isArray(record.tags)
			? record.tags.filter((tag): tag is string => typeof tag === 'string')
			: undefined,
	}
}
