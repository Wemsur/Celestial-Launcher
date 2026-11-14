import { reactive } from 'vue'

import { mirrorById } from '@/helpers/github-mirror'
import {
	isMinecraftDownloadSourcePreference,
	loadUiPreferences,
	saveUiPreferences,
	UI_PREFERENCE_DEFAULTS,
	type UiPreferencesFile,
} from '@/helpers/ui-preferences-store'

type UiPreferences = Omit<UiPreferencesFile, 'version'>

/**
 * Interface preferences use the `ui_preferences` document in `celestial.db.ui_settings`.
 *
 * One module-level reactive object, so every reader sees the same values and a
 * change from the settings modal reaches the sidebar without an event.
 */
const preferences = reactive<UiPreferences>({ ...UI_PREFERENCE_DEFAULTS })

let load: Promise<void> | null = null
let saveQueue: Promise<void> = Promise.resolve()
/** Set by the first successful write, so a read cannot undo a saved change. */
let dirty = false

/**
 * Copy one stored field over its default only when its type and allowed values
 * match. Older or hand-edited documents must not replace valid defaults.
 */
function adopt<K extends keyof UiPreferences>(key: K, value: unknown): void {
	if (key === 'minecraftDownloadSourcePreference' && !isMinecraftDownloadSourcePreference(value)) {
		return
	}

	if (typeof value === typeof UI_PREFERENCE_DEFAULTS[key]) {
		preferences[key] = value as UiPreferences[K]
	}
}

export function ensureUiPreferencesLoaded(): Promise<void> {
	load ??= loadUiPreferences().then((stored) => {
		if (!stored || dirty) return

		for (const key of Object.keys(UI_PREFERENCE_DEFAULTS) as (keyof UiPreferences)[]) {
			adopt(key, stored[key])
		}

		// A mirror id that is no longer offered falls back to the default rather
		// than leaving the store pointed at nothing.
		if (!mirrorById(preferences.githubMirrorId)) {
			preferences.githubMirrorId = UI_PREFERENCE_DEFAULTS.githubMirrorId
		}
	})

	return load
}

/** Reactive preferences. Kicks off the one-time read on first use. */
export function useUiPreferences(): UiPreferences {
	void ensureUiPreferencesLoaded()
	return preferences
}

/**
 * The same object, without kicking off a load. For modules that read a
 * preference outside a component setup — the store fetchers, say — where
 * calling a composable would be misleading.
 */
export const uiPreferences = preferences

export async function setUiPreferences(patch: Partial<UiPreferences>): Promise<void> {
	const pendingPatch = { ...patch }
	await ensureUiPreferencesLoaded()

	const save = saveQueue.then(async () => {
		const candidate: UiPreferences = { ...preferences, ...pendingPatch }
		await saveUiPreferences(candidate)
		Object.assign(preferences, candidate)
		dirty = true
	})

	saveQueue = save.catch(() => {})
	await save
}
