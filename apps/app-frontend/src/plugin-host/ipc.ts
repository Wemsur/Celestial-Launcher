/** Typed wrappers around the `plugin:plugins|*` commands. */

import { convertFileSrc, invoke } from '@tauri-apps/api/core'

import type { PluginSummary } from './types'

export function listPlugins(): Promise<PluginSummary[]> {
	return invoke<PluginSummary[]>('plugin:plugins|plugin_list')
}

export function getPlugin(pluginId: string): Promise<PluginSummary | null> {
	return invoke<PluginSummary | null>('plugin:plugins|plugin_get', { pluginId })
}

export function setPluginEnabled(pluginId: string, enabled: boolean): Promise<PluginSummary> {
	return invoke<PluginSummary>('plugin:plugins|plugin_set_enabled', { pluginId, enabled })
}

export function installPlugin(path: string, origin?: string): Promise<PluginSummary> {
	// `origin` is an Option on the Rust side; send an explicit null rather than
	// relying on a missing key deserialising to None.
	return invoke<PluginSummary>('plugin:plugins|plugin_install', { path, origin: origin ?? null })
}

/** Download a zipped plugin from a store entry and install it. */
export function installPluginFromUrl(url: string, origin?: string): Promise<PluginSummary> {
	return invoke<PluginSummary>('plugin:plugins|plugin_install_from_url', {
		url,
		origin: origin ?? null,
	})
}

/**
 * Download a zipped plugin from a store entry and update the installed copy,
 * keeping its enabled state and granted permissions.
 */
export function updatePluginFromUrl(url: string, origin?: string): Promise<PluginSummary> {
	return invoke<PluginSummary>('plugin:plugins|plugin_update_from_url', {
		url,
		origin: origin ?? null,
	})
}

/**
 * The tag of a store plugin's latest GitHub release (`owner/name`), or null
 * when the repo has no published release. Read from the `releases/latest`
 * redirect, so it does not spend the GitHub API rate limit.
 */
export function pluginLatestRelease(repo: string): Promise<string | null> {
	return invoke<string | null>('plugin:plugins|plugin_latest_release', { repo })
}

export function uninstallPlugin(pluginId: string, removeData = false): Promise<void> {
	return invoke('plugin:plugins|plugin_uninstall', { pluginId, removeData })
}

export function setPluginGranted(pluginId: string, granted: string[]): Promise<PluginSummary> {
	return invoke<PluginSummary>('plugin:plugins|plugin_set_granted', { pluginId, granted })
}

export function setPluginPinnedPages(pluginId: string, pages: string[]): Promise<PluginSummary> {
	return invoke<PluginSummary>('plugin:plugins|plugin_set_pinned_pages', { pluginId, pages })
}

export function grantPluginPermission(pluginId: string, permission: string): Promise<PluginSummary> {
	return invoke<PluginSummary>('plugin:plugins|plugin_grant_permission', { pluginId, permission })
}

export function revokePluginPermission(pluginId: string, permission: string): Promise<PluginSummary> {
	return invoke<PluginSummary>('plugin:plugins|plugin_revoke_permission', { pluginId, permission })
}

export function openPluginFolder(pluginId?: string): Promise<void> {
	return invoke('plugin:plugins|plugin_open_folder', { pluginId: pluginId ?? null })
}

export function pluginStorageGet(pluginId: string, key: string): Promise<string | null> {
	return invoke<string | null>('plugin:plugins|plugin_storage_get', { pluginId, key })
}

export function pluginStorageSet(pluginId: string, key: string, value: string): Promise<void> {
	return invoke('plugin:plugins|plugin_storage_set', { pluginId, key, value })
}

export function pluginStorageRemove(pluginId: string, key: string): Promise<void> {
	return invoke('plugin:plugins|plugin_storage_remove', { pluginId, key })
}

export function pluginStorageKeys(pluginId: string): Promise<string[]> {
	return invoke<string[]>('plugin:plugins|plugin_storage_keys', { pluginId })
}

export function pluginCrashLog(pluginId: string): Promise<string | null> {
	return invoke<string | null>('plugin:plugins|plugin_crash_log', { pluginId })
}

export function pluginReportCrash(
	pluginId: string,
	message: string,
	stack?: string,
): Promise<void> {
	return invoke('plugin:plugins|plugin_report_crash', { pluginId, message, stack })
}

/**
 * Fallback source for the loader: the asset protocol can refuse a
 * cross-origin module load, in which case the entry is imported from a blob
 * URL built out of this text.
 */
export function pluginReadEntry(pluginId: string): Promise<string | null> {
	return invoke<string | null>('plugin:plugins|plugin_read_entry', { pluginId })
}

export interface PluginFetchRequest {
	url: string
	method?: string
	headers?: Record<string, string>
	body?: string
}

export interface PluginFetchResponse {
	status: number
	ok: boolean
	headers: Record<string, string>
	body: string
}

/** Network request proxied through Rust; host is checked there against grants. */
export function pluginNetworkFetch(
	pluginId: string,
	request: PluginFetchRequest,
): Promise<PluginFetchResponse> {
	return invoke<PluginFetchResponse>('plugin:plugins|plugin_fetch', {
		pluginId,
		request,
	})
}

/** Declared-settings values, read for the plugin page's form. */
export function pluginSettingsGet(pluginId: string): Promise<Record<string, string>> {
	return invoke<Record<string, string>>('plugin:plugins|plugin_settings_get', { pluginId })
}

/** Save one declared-settings value from the plugin page. */
export function pluginSettingsSet(
	pluginId: string,
	key: string,
	value: string,
): Promise<void> {
	return invoke('plugin:plugins|plugin_settings_set', { pluginId, key, value })
}

/** Whether plugin state changes apply to the running launcher without a restart. */
export function pluginGetHotReload(): Promise<boolean> {
	return invoke<boolean>('plugin:plugins|plugin_get_hot_reload')
}

/** Turn live application of plugin state changes on or off. */
export function pluginSetHotReload(enabled: boolean): Promise<void> {
	return invoke('plugin:plugins|plugin_set_hot_reload', { enabled })
}

export interface SidecarStarted {
	handle: number
	port: number | null
}

export interface SidecarRequestInit {
	/** Path plus query string, used verbatim (so repeated query keys work). */
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
	version: string | null
}

/** Download, verify, and unpack a sidecar binary. Requires `sidecar`. */
export function pluginSidecarEnsure(
	pluginId: string,
	key: string,
	url: string,
	options?: { sha512?: string; sha512Url?: string; archive?: string; version?: string },
): Promise<void> {
	return invoke('plugin:plugins|plugin_sidecar_ensure', {
		pluginId,
		key,
		url,
		sha512: options?.sha512 ?? null,
		sha512Url: options?.sha512Url ?? null,
		archive: options?.archive ?? null,
		version: options?.version ?? null,
	})
}

/** Whether a sidecar is installed for the plugin, and its recorded version. */
export function pluginSidecarStatus(pluginId: string, key: string): Promise<SidecarStatus> {
	return invoke<SidecarStatus>('plugin:plugins|plugin_sidecar_status', { pluginId, key })
}

/** Spawn an installed sidecar; returns its handle and (if requested) its port. */
export function pluginSidecarStart(
	pluginId: string,
	key: string,
	args: string[],
	portFile = false,
): Promise<SidecarStarted> {
	return invoke<SidecarStarted>('plugin:plugins|plugin_sidecar_start', {
		pluginId,
		key,
		args,
		portFile,
	})
}

/** Proxy a control request to a running sidecar over localhost. */
export function pluginSidecarRequest(
	pluginId: string,
	handle: number,
	request: SidecarRequestInit,
): Promise<SidecarResponse> {
	return invoke<SidecarResponse>('plugin:plugins|plugin_sidecar_request', {
		pluginId,
		handle,
		request,
	})
}

export function pluginSidecarStop(pluginId: string, handle: number): Promise<void> {
	return invoke('plugin:plugins|plugin_sidecar_stop', { pluginId, handle })
}

export function pluginSidecarCleanup(pluginId: string): Promise<void> {
	return invoke('plugin:plugins|plugin_sidecar_cleanup', { pluginId })
}

/** Start announcing a Minecraft LAN world. Requires `lan`. */
export function pluginLanAnnounce(
	pluginId: string,
	motd: string,
	port: number,
): Promise<{ handle: number }> {
	return invoke<{ handle: number }>('plugin:plugins|plugin_lan_announce', {
		pluginId,
		motd,
		port,
	})
}

export function pluginLanStop(pluginId: string, handle: number): Promise<void> {
	return invoke('plugin:plugins|plugin_lan_stop', { pluginId, handle })
}

export function pluginLanCleanup(pluginId: string): Promise<void> {
	return invoke('plugin:plugins|plugin_lan_cleanup', { pluginId })
}

/**
 * Turn an absolute path inside the plugins folder into a URL the webview may
 * load. `tauri.conf.json` scopes the asset protocol to `$APPDATA/plugins/**`
 * and allows `asset:` in `script-src`; without both, this URL is refused by CSP
 * and the import fails.
 *
 * The query string is not decoration: the webview caches asset responses on
 * disk across restarts, so a plugin that was edited or reinstalled kept running
 * its previous bundle. A distinct URL per load is what makes the file on disk
 * the file that actually executes.
 */
export function pluginAssetUrl(absolutePath: string): string {
	return `${convertFileSrc(absolutePath)}?v=${Date.now()}`
}
