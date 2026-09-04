import { reactive } from 'vue'

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

export function ensureUiPreferencesLoaded(): Promise<void> {
	load ??= loadUiPreferences().then((stored) => {
		if (!stored || dirty) return

		for (const key of Object.keys(UI_PREFERENCE_DEFAULTS) as (keyof UiPreferences)[]) {
			const value = stored[key]
			if (typeof value === 'boolean') preferences[key] = value
		}
	})

	return load
}

/** Reactive preferences. Kicks off the one-time read on first use. */
export function useUiPreferences(): UiPreferences {
	void ensureUiPreferencesLoaded()
	return preferences
}

export async function setUiPreferences(patch: Partial<UiPreferences>): Promise<void> {
	dirty = true
	Object.assign(preferences, patch)
	await saveUiPreferences({ ...preferences })
}
