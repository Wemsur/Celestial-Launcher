/**
 * Loads content-source plugins and collects the providers they register.
 *
 * A content source is not a launcher UI plugin: it contributes no UI and is
 * given a much smaller host API — reach the network, keep a little storage, log,
 * and register one provider. Everything else (the manifest, the folder, the
 * enable flag, the network allow-list) lives in the backend, which is also where
 * a source's reach is actually enforced; the object here is the intended path,
 * not a sandbox.
 *
 * A source that throws while activating is recorded and switched off, the same
 * way a crashing UI plugin is, so it cannot throw again on every browse.
 */

import {
	listSources,
	sourceAssetUrl,
	sourceFetch,
	sourceReadEntry,
	sourceReportCrash,
	sourceSettingsGet,
	sourceStorageGet,
	sourceStorageKeys,
	sourceStorageRemove,
	sourceStorageSet,
} from './ipc'
import {
	clearSourceProviders,
	registerSourceProvider,
	setContentSourceSummaries,
	setSourceLoadError,
} from './registry'
import i18n from '@/i18n.config'

import type {
	ContentSourceManifest,
	ContentSourceProvider,
	SourceSummary,
} from './types'

/** The host API handed to a content source's `activate`. */
export interface ContentSourceApi {
	readonly i18n: { readonly locale: string }
	readonly source: {
		readonly id: string
		readonly name: string
		readonly version: string
		readonly manifest: ContentSourceManifest
	}
	/**
	 * Fetch through the launcher. The host must be one the source declared in
	 * its manifest; anything else is refused backend-side. Unlike the UI-plugin
	 * proxy this permits `cookie`, `referer`, `user-agent` and `authorization`,
	 * which content sites routinely require.
	 */
	readonly net: {
		fetch(url: string, options?: SourceFetchOptions): Promise<SourceFetchResult>
	}
	readonly storage: {
		get(key: string): Promise<string | null>
		set(key: string, value: string): Promise<void>
		remove(key: string): Promise<void>
		keys(): Promise<string[]>
	}
	/** Read-only values of the source's declared settings (from its manifest). */
	readonly settings: {
		get(key: string): Promise<string | null>
		all(): Promise<Record<string, string>>
	}
	/** Register this source's provider. Call it once, from `activate`. */
	register(provider: ContentSourceProvider): void
	log(...args: unknown[]): void
}

export interface SourceFetchOptions {
	method?: string
	headers?: Record<string, string>
	body?: string
}

export interface SourceFetchResult {
	status: number
	ok: boolean
	headers: Record<string, string>
	body: string
	final_url?: string | null
}

interface SourceModule {
	activate?: (api: ContentSourceApi) => void | Promise<void>
	default?: {
		activate?: (api: ContentSourceApi) => void | Promise<void>
	}
}

/** The in-flight load, so repeated calls while loading are coalesced. */
let loading: Promise<void> | null = null

export interface LoadContentSourcesOptions {
	/** Called when a source throws while starting, after it has been disabled. */
	onCrash?: (summary: SourceSummary, message: string) => void
}

/** The options the in-flight load was started with. */
let currentOptions: LoadContentSourcesOptions = {}

/**
 * Load (or reload) every enabled content source. Safe to call repeatedly: while
 * a load is in flight, further calls wait for it rather than starting a second.
 */
export function loadContentSources(
	options: LoadContentSourcesOptions = {},
): Promise<void> {
	if (!loading) {
		currentOptions = options
		loading = runLoad().finally(() => {
			loading = null
		})
	}
	return loading
}

async function runLoad(): Promise<void> {
	let summaries: SourceSummary[]
	try {
		summaries = await listSources()
	} catch (error) {
		console.error('[content-sources] could not list sources', error)
		return
	}

	setContentSourceSummaries(summaries)
	clearSourceProviders()
	const enabled = summaries.filter(
		(summary) => summary.enabled && summary.manifest,
	)
	await Promise.all(enabled.map((summary) => loadOne(summary)))
}

async function loadOne(summary: SourceSummary): Promise<void> {
	const manifest = summary.manifest
	if (!manifest) return

	try {
		const activate = await importActivate(summary.path, manifest.entry)
		if (typeof activate !== 'function') {
			throw new Error(
				`Content source "${summary.id}" has no activate() export`,
			)
		}

		const registered: ContentSourceProvider[] = []
		const api = createApi(summary, manifest, (provider) => {
			registered.push(provider)
		})

		await activate(api)

		const provider = registered[0]
		if (!provider) {
			// Activating without registering is not fatal — the source simply
			// contributes nothing — but say so, because it is almost always a bug.
			console.warn(
				`[content-sources] "${summary.id}" activated without registering a provider`,
			)
			return
		}
		if (registered.length > 1) {
			// One provider per source is the contract; extra registrations would
			// be silently dropped, so name them rather than losing them quietly.
			console.warn(
				`[content-sources] "${summary.id}" registered ${registered.length} providers; only the first is used`,
			)
		}
		registerSourceProvider(summary.id, provider)
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error)
		const stack = error instanceof Error ? error.stack : undefined
		console.error(`[content-sources] "${summary.id}" failed to load`, error)
		setSourceLoadError(summary.id, message)
		currentOptions.onCrash?.(summary, message)
		try {
			await sourceReportCrash(summary.id, message, stack)
		} catch {
			// Reporting is best-effort; the source is already off this session.
		}
	}
}

/**
 * Import a source's entry module and return its `activate`.
 *
 * The asset protocol is the fast path; when it refuses the cross-origin load
 * the entry text is fetched instead and imported from a blob URL, which is
 * same-origin and always allowed.
 */
async function importActivate(
	dir: string,
	entry: string,
): Promise<SourceModule['activate']> {
	const path = joinPath(dir, entry)

	try {
		const module = (await import(/* @vite-ignore */ sourceAssetUrl(path))) as SourceModule
		return module.activate ?? module.default?.activate
	} catch {
		const text = await sourceReadEntry(dirBasename(dir))
		if (!text) throw new Error('Entry could not be read')
		const blob = new Blob([text], { type: 'text/javascript' })
		const url = URL.createObjectURL(blob)
		try {
			const module = (await import(/* @vite-ignore */ url)) as SourceModule
			return module.activate ?? module.default?.activate
		} finally {
			URL.revokeObjectURL(url)
		}
	}
}

function createApi(
	summary: SourceSummary,
	manifest: ContentSourceManifest,
	onRegister: (provider: ContentSourceProvider) => void,
): ContentSourceApi {
	const id = summary.id
	return Object.freeze({
		i18n: Object.freeze({
			get locale() {
				return i18n.global.locale.value
			},
		}),
		source: Object.freeze({
			id,
			name: manifest.name,
			version: manifest.version,
			manifest,
		}),
		net: Object.freeze({
			fetch: (url: string, options?: SourceFetchOptions) =>
				sourceFetch(id, {
					url,
					method: options?.method,
					headers: options?.headers,
					body: options?.body,
				}),
		}),
		storage: Object.freeze({
			get: (key: string) => sourceStorageGet(id, key),
			set: (key: string, value: string) => sourceStorageSet(id, key, value),
			remove: (key: string) => sourceStorageRemove(id, key),
			keys: () => sourceStorageKeys(id),
		}),
		settings: Object.freeze({
			get: async (key: string) => {
				const all = await sourceSettingsGet(id)
				return all[key] ?? null
			},
			all: () => sourceSettingsGet(id),
		}),
		register: (provider: ContentSourceProvider) => onRegister(provider),
		log: (...args: unknown[]) => console.log(`[content-source:${id}]`, ...args),
	})
}

/** Join a directory and a relative entry, tolerating either separator. */
function joinPath(dir: string, entry: string): string {
	const separator = dir.includes('\\') ? '\\' : '/'
	return `${dir.replace(/[\\/]+$/, '')}${separator}${entry.replace(/^[\\/]+/, '')}`
}

/** The last path segment, which is the source's folder name (its id). */
function dirBasename(dir: string): string {
	const parts = dir.split(/[\\/]+/).filter(Boolean)
	return parts[parts.length - 1] ?? dir
}
