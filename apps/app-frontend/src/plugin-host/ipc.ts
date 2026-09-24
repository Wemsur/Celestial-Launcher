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

export function uninstallPlugin(pluginId: string, removeData = false): Promise<void> {
	return invoke('plugin:plugins|plugin_uninstall', { pluginId, removeData })
}

export function setPluginGranted(pluginId: string, granted: string[]): Promise<PluginSummary> {
	return invoke<PluginSummary>('plugin:plugins|plugin_set_granted', { pluginId, granted })
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
