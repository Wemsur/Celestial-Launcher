/** Typed wrappers around the `plugin:content-sources|*` commands. */

import { convertFileSrc, invoke } from '@tauri-apps/api/core'

import type { SourceSummary } from './types'

export function listSources(): Promise<SourceSummary[]> {
	return invoke<SourceSummary[]>('plugin:content-sources|content_source_list')
}

export function getSource(sourceId: string): Promise<SourceSummary | null> {
	return invoke<SourceSummary | null>('plugin:content-sources|content_source_get', {
		sourceId,
	})
}

export function installSource(path: string, origin?: string): Promise<SourceSummary> {
	return invoke<SourceSummary>('plugin:content-sources|content_source_install', {
		path,
		origin: origin ?? null,
	})
}

export function installSourceFromUrl(url: string, origin?: string): Promise<SourceSummary> {
	return invoke<SourceSummary>('plugin:content-sources|content_source_install_from_url', {
		url,
		origin: origin ?? null,
	})
}

export function updateSourceFromUrl(url: string, origin?: string): Promise<SourceSummary> {
	return invoke<SourceSummary>('plugin:content-sources|content_source_update_from_url', {
		url,
		origin: origin ?? null,
	})
}

/**
 * The tag of a source's latest GitHub release (`owner/name`), or null when the
 * repo has no published release. Read from the `releases/latest` redirect, so it
 * does not spend the GitHub API rate limit.
 */
export function sourceLatestRelease(repo: string): Promise<string | null> {
	return invoke<string | null>('plugin:content-sources|content_source_latest_release', {
		repo,
	})
}

export function uninstallSource(sourceId: string, removeData = false): Promise<void> {
	return invoke('plugin:content-sources|content_source_uninstall', { sourceId, removeData })
}

export function setSourceEnabled(sourceId: string, enabled: boolean): Promise<SourceSummary> {
	return invoke<SourceSummary>('plugin:content-sources|content_source_set_enabled', {
		sourceId,
		enabled,
	})
}

export function openSourceFolder(sourceId?: string): Promise<void> {
	return invoke('plugin:content-sources|content_source_open_folder', {
		sourceId: sourceId ?? null,
	})
}

export function sourceStorageGet(sourceId: string, key: string): Promise<string | null> {
	return invoke<string | null>('plugin:content-sources|content_source_storage_get', {
		sourceId,
		key,
	})
}

export function sourceStorageSet(sourceId: string, key: string, value: string): Promise<void> {
	return invoke('plugin:content-sources|content_source_storage_set', { sourceId, key, value })
}

export function sourceStorageRemove(sourceId: string, key: string): Promise<void> {
	return invoke('plugin:content-sources|content_source_storage_remove', { sourceId, key })
}

export function sourceStorageKeys(sourceId: string): Promise<string[]> {
	return invoke<string[]>('plugin:content-sources|content_source_storage_keys', { sourceId })
}

/** Declared-settings values, read for the source manager's form. */
export function sourceSettingsGet(sourceId: string): Promise<Record<string, string>> {
	return invoke<Record<string, string>>('plugin:content-sources|content_source_settings_get', {
		sourceId,
	})
}

/** Save one declared-settings value from the source manager. */
export function sourceSettingsSet(sourceId: string, key: string, value: string): Promise<void> {
	return invoke('plugin:content-sources|content_source_settings_set', { sourceId, key, value })
}

export function sourceCrashLog(sourceId: string): Promise<string | null> {
	return invoke<string | null>('plugin:content-sources|content_source_crash_log', { sourceId })
}

/** Called by the loader when a source throws, so the backend can disable it. */
export function sourceReportCrash(
	sourceId: string,
	message: string,
	stack?: string,
): Promise<void> {
	return invoke('plugin:content-sources|content_source_report_crash', {
		sourceId,
		message,
		stack,
	})
}

/**
 * Fallback source for the loader: the asset protocol can refuse a cross-origin
 * module load, in which case the entry is imported from a blob URL built out of
 * this text.
 */
export function sourceReadEntry(sourceId: string): Promise<string | null> {
	return invoke<string | null>('plugin:content-sources|content_source_read_entry', {
		sourceId,
	})
}

export interface SourceFetchRequest {
	url: string
	method?: string
	headers?: Record<string, string>
	body?: string
}

export interface SourceFetchResponse {
	status: number
	ok: boolean
	headers: Record<string, string>
	body: string
	final_url?: string | null
}

/** Network request proxied through Rust; the host is checked there against the
 *  source's declared hosts. */
export function sourceFetch(
	sourceId: string,
	request: SourceFetchRequest,
): Promise<SourceFetchResponse> {
	return invoke<SourceFetchResponse>('plugin:content-sources|content_source_fetch', {
		sourceId,
		request,
	})
}

/**
 * Turn an absolute path inside the content-sources folder into a URL the
 * webview may load. `tauri.conf.json` scopes the asset protocol to
 * `$APPDATA/content-sources/**` and allows `asset:` in `script-src`; without
 * both, this URL is refused by CSP and the import fails.
 *
 * The query string is not decoration: the webview caches asset responses on
 * disk across restarts, so a source that was edited or reinstalled kept running
 * its previous bundle. A distinct URL per load is what makes the file on disk
 * the file that actually executes.
 */
export function sourceAssetUrl(absolutePath: string): string {
	return `${convertFileSrc(absolutePath)}?v=${Date.now()}`
}

/**
 * Resolve a file inside a source's own folder — its icon, say — to a URL the
 * webview may load. The relative part comes from the manifest and has already
 * been checked backend-side to stay inside the folder.
 *
 * Deliberately without the cache-busting query `sourceAssetUrl` carries: this
 * is also how a source's icon is shown in lists, and a URL that changed on
 * every render would make the browser re-fetch the same image over and over.
 */
export function contentSourceAssetUrl(
	sourceDir: string,
	relativePath: string,
): string {
	const separator = sourceDir.includes('\\') ? '\\' : '/'
	const joined = `${sourceDir.replace(/[\\/]+$/, '')}${separator}${relativePath.replace(
		/^[\\/]+/,
		'',
	)}`
	return convertFileSrc(joined)
}

export interface InstallDescriptor {
	url: string
	headers?: Record<string, string>
	fileName?: string
	hash?: { algorithm: string; value: string }
}

export interface InstalledContent {
	/** Relative to the instance, or to the launcher's data dir for a skin. */
	relative_path: string
	file_name: string
	/** The install record's id, absent for a skin (which is not tracked). */
	record_id?: string | null
}

/** Where an install came from, recorded so a later update check can ask about it. */
export interface InstallMetadata {
	sourceId: string
	projectId: string
	versionId?: string
	versionName?: string
	name?: string
	iconUrl?: string
	installRule?: Record<string, unknown>
}

/** One installed file remembered for an instance. */
export interface SourceContentRecord {
	id: string
	source_id: string
	project_id: string
	version_id?: string | null
	version_name?: string | null
	content_type: string
	relative_path: string
	name: string
	icon_url?: string | null
	install_rule?: Record<string, unknown> | null
	installed_at?: number | null
}

/**
 * Install one file a content source produced. The launcher decides where it
 * lands, by content type; `instanceId` is unused for a skin, which belongs to
 * the launcher rather than to an instance.
 */
export function installSourceContent(
	instanceId: string | undefined,
	contentType: string,
	descriptor: InstallDescriptor,
	meta?: InstallMetadata,
): Promise<InstalledContent> {
	return invoke<InstalledContent>('plugin:content-sources|content_source_install_content', {
		instanceId: instanceId ?? null,
		contentType,
		descriptor,
		meta: meta ?? null,
	})
}

/** Everything installed from a content source into an instance. */
export function listInstalledSourceContent(
	instanceId: string,
): Promise<SourceContentRecord[]> {
	return invoke<SourceContentRecord[]>('plugin:content-sources|content_source_list_installed', {
		instanceId,
	})
}

/** Forget an install record, leaving the file on disk alone. */
export function forgetInstalledSourceContent(
	instanceId: string,
	recordId: string,
): Promise<void> {
	return invoke('plugin:content-sources|content_source_forget_installed', {
		instanceId,
		recordId,
	})
}

export interface DownloadedSkin {
	name: string
	/** Absolute path, for `convertFileSrc`. */
	path: string
}

/** Skins downloaded through content sources, shown on the skins page. */
export function listDownloadedSkins(): Promise<DownloadedSkin[]> {
	return invoke<DownloadedSkin[]>('plugin:content-sources|content_source_list_downloaded_skins')
}
