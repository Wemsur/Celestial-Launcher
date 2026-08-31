// Disk persistence for the discover split view switch.
//
// Its own file, `<appdata>/interface/split-view.json`, written through the Rust
// commands in `apps/app/src/main.rs` — never localStorage, so the setting
// survives a webview data wipe like every other launcher preference.
import { invoke } from '@tauri-apps/api/core'

export interface SplitViewSettingsFile {
	version: 1
	/** Whether a discover card should open beside the list instead of full width. */
	enabled: boolean
}

/** `null` when nothing has been saved yet, so the caller keeps its default. */
export async function loadSplitViewEnabled(): Promise<boolean | null> {
	try {
		const raw = await invoke<string>('load_split_view_settings')
		if (!raw) return null

		const parsed = JSON.parse(raw) as Partial<SplitViewSettingsFile>
		return typeof parsed?.enabled === 'boolean' ? parsed.enabled : null
	} catch (error) {
		console.error('Failed to load split view settings:', error)
		return null
	}
}

export async function saveSplitViewEnabled(enabled: boolean): Promise<void> {
	const file: SplitViewSettingsFile = { version: 1, enabled }

	try {
		await invoke('save_split_view_settings', { contents: JSON.stringify(file, null, 2) })
	} catch (error) {
		console.error('Failed to save split view settings:', error)
	}
}
