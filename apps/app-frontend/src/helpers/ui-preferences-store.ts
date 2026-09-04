// Disk persistence for interface preferences that deliberately stay out of the
// database.
//
// Its own file, `<appdata>/interface/ui-preferences.json`, written through the
// Rust commands in `apps/app/src/main.rs`. Two reasons it is not part of
// `settings.feature_flags`:
//
//  - `feature_flags` is a `HashMap<FeatureFlag, bool>` behind a strict Rust enum.
//    An unknown key makes the whole map fail to deserialize, and the call site
//    falls back to `unwrap_or_default()` — silently wiping every other flag.
//  - Adding a flag there means touching the DB-backed settings type on every
//    change. A frontend-owned JSON file needs no Rust change to grow.
//
// Not localStorage either, so preferences survive a webview data wipe.
import { invoke } from '@tauri-apps/api/core'

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
}

export const UI_PREFERENCE_DEFAULTS: Omit<UiPreferencesFile, 'version'> = {
	jumpBackInSidebar: true,
	sidebarVisibleOnStartup: true,
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
	}
}
