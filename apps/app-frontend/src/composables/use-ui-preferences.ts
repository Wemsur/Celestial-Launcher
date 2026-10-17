import { reactive } from 'vue'

import { mirrorById } from '@/helpers/github-mirror'
import {
	loadUiPreferences,
	saveUiPreferences,
	UI_PREFERENCE_DEFAULTS,
	type UiPreferencesFile,
} from '@/helpers/ui-preferences-store'

type UiPreferences = Omit<UiPreferencesFile, 'version'>

/**
 * Interface preferences held in `<appdata>/interface/ui-preferences.json` rather
 * than the settings database — see the store module for why.
 *
 * One module-level reactive object, so every reader sees the same values and a
 * change from the settings modal reaches the sidebar without an event.
 */
const preferences = reactive<UiPreferences>({ ...UI_PREFERENCE_DEFAULTS })

let load: Promise<void> | null = null
/** Set by the first write, so a save cannot be undone by a read still in flight. */
let dirty = false

/**
 * Copy one stored field over its default, but only when the file's value has
 * the same type the default does. A hand-edited or older file must not be able
 * to put a string where a toggle goes, or the reverse.
 */
function adopt<K extends keyof UiPreferences>(key: K, value: unknown): void {
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
	dirty = true
	Object.assign(preferences, patch)
	await saveUiPreferences({ ...preferences })
}
