/**
 * Interface preferences use the `ui_preferences` document in
 * `celestial.db.ui_settings`, through the existing load/save commands.
 * They stay separate from `AppSettings` and its strictly typed feature flags,
 * and survive a webview data wipe.
 */
import { invoke } from '@tauri-apps/api/core'

import { DEFAULT_MIRROR_ID } from '@/helpers/github-mirror'

export const MINECRAFT_DOWNLOAD_SOURCE_PREFERENCES = [
	'auto',
	'official_first',
	'mirror_first',
] as const

export type MinecraftDownloadSourcePreference =
	(typeof MINECRAFT_DOWNLOAD_SOURCE_PREFERENCES)[number]

export function isMinecraftDownloadSourcePreference(
	value: unknown,
): value is MinecraftDownloadSourcePreference {
	return MINECRAFT_DOWNLOAD_SOURCE_PREFERENCES.some((preference) => preference === value)
}

export interface UiPreferencesFile {
	version: 1
	/** Compact "Jump in" list at the top of the right sidebar, above the play account. */
	jumpBackInSidebar: boolean
	/**
	 * Whether the right sidebar starts open. Only the *initial* state — clicking the
	 * collapse button afterwards is not written back, so the setting stays a
	 * statement about launch rather than a live mirror of the sidebar.
	 */
	sidebarVisibleOnStartup: boolean
	/**
	 * Whether the two stores reach GitHub through a mirror. Direct access to
	 * `raw.githubusercontent.com` and release assets times out on some networks,
	 * so the store indexes and their downloads can be routed through a proxy.
	 */
	githubMirrorEnabled: boolean
	/** Which mirror to use, by its id in `helpers/github-mirror.ts`. */
	githubMirrorId: string
	/** Preferred source order for Minecraft file downloads, with fallback allowed. */
	minecraftDownloadSourcePreference: MinecraftDownloadSourcePreference
}

export const UI_PREFERENCE_DEFAULTS: Omit<UiPreferencesFile, 'version'> = {
	jumpBackInSidebar: true,
	sidebarVisibleOnStartup: true,
	// Off by default: a mirror is a third party, and the store works without one
	// wherever GitHub itself is reachable.
	githubMirrorEnabled: false,
	githubMirrorId: DEFAULT_MIRROR_ID,
	minecraftDownloadSourcePreference: 'auto',
}

/** `null` when nothing has been saved yet, so the caller keeps its defaults. */
export async function loadUiPreferences(): Promise<Partial<UiPreferencesFile> | null> {
	try {
		const raw = await invoke<string>('load_ui_preferences')
		if (!raw) return null

		const parsed = JSON.parse(raw) as unknown
		return parsed && typeof parsed === 'object' ? (parsed as Partial<UiPreferencesFile>) : null
	} catch (error) {
		console.error('Failed to load UI preferences:', error)
		return null
	}
}

export async function saveUiPreferences(
	preferences: Omit<UiPreferencesFile, 'version'>,
): Promise<void> {
	const file: UiPreferencesFile = { version: 1, ...preferences }

	try {
		await invoke('save_ui_preferences', { contents: JSON.stringify(file, null, 2) })
	} catch (error) {
		console.error('Failed to save UI preferences:', error)
		throw error
	}
}
