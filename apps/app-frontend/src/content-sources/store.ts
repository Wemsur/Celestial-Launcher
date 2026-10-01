/**
 * The content-source store: a `content-sources.json` index of installable
 * source plugins, fetched and rendered as an install list.
 *
 * Like the plugin store, the index is third-party data — fetched read-only and
 * validated entry by entry. Installing a source goes through the same plugin
 * install path as any other plugin, so its manifest and permissions are still
 * the user's to approve. The two stores are expected to converge; keeping the
 * index separate for now lets sources be curated on their own.
 */

const DEFAULT_REGISTRY_URL =
	'https://raw.githubusercontent.com/Wemsur/Celestial-Plugin-Store/refs/heads/main/content-sources.json'

const REGISTRY_URL_KEY = 'celestial-content-source-store-url'

export interface StoreContentSource {
	id: string
	name: string
	description?: string
	author?: string
	/** `owner/name` of the plugin's GitHub repo; drives update detection. */
	github?: string
	/** The source's own page/repo, shown as a link. */
	repo?: string
	/** Direct download of the packaged plugin (a .zip). */
	download: string
	icon?: string
	/** Content types the source serves, for display before install. */
	contentTypes?: string[]
	tags?: string[]
}

interface RegistryFile {
	version?: number
	sources?: unknown[]
}

/** Where the store index is fetched from. Overridable for a fork or staging index. */
export function sourceRegistryUrl(): string {
	if (typeof localStorage === 'undefined') {
		return DEFAULT_REGISTRY_URL
	}
	return localStorage.getItem(REGISTRY_URL_KEY) || DEFAULT_REGISTRY_URL
}

export function setSourceRegistryUrl(url: string): void {
	if (typeof localStorage === 'undefined') return
	if (url.trim()) {
		localStorage.setItem(REGISTRY_URL_KEY, url.trim())
	} else {
		localStorage.removeItem(REGISTRY_URL_KEY)
	}
}

export async function fetchStoreSources(): Promise<StoreContentSource[]> {
	const response = await fetch(sourceRegistryUrl(), {
		headers: { accept: 'application/json' },
	})
	if (!response.ok) {
		throw new Error(`内容源商店请求失败：HTTP ${response.status}`)
	}

	const data = (await response.json()) as RegistryFile
	if (!data || !Array.isArray(data.sources)) {
		throw new Error('内容源商店索引格式无效：缺少 sources 数组')
	}

	return data.sources
		.map(normalizeEntry)
		.filter((entry): entry is StoreContentSource => entry !== null)
}

/** Coerce and vet one raw registry entry; drops anything unusable. */
function normalizeEntry(raw: unknown): StoreContentSource | null {
	if (!raw || typeof raw !== 'object') return null
	const record = raw as Record<string, unknown>

	const id = typeof record.id === 'string' ? record.id : null
	const name = typeof record.name === 'string' ? record.name : null
	const download = typeof record.download === 'string' ? record.download : null
	if (!id || !name || !download) return null

	if (!/^https?:\/\//i.test(download)) return null

	const str = (value: unknown) => (typeof value === 'string' ? value : undefined)
	const strList = (value: unknown) =>
		Array.isArray(value)
			? value.filter((item): item is string => typeof item === 'string')
			: undefined

	return {
		id,
		name,
		download,
		description: str(record.description),
		author: str(record.author),
		github: str(record.github),
		repo: str(record.repo),
		icon: str(record.icon),
		contentTypes: strList(record.contentTypes),
		tags: strList(record.tags),
	}
}
