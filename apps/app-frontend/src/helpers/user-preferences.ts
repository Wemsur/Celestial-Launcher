import type { UserPreferencesContext } from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'

type UserPreferences = NonNullable<UserPreferencesContext['preferences']['value']>
type PartialUserPreferences = Parameters<UserPreferencesContext['updatePreferences']>[0]

/**
 * The last preferences each account was seen with, so a write this build
 * refuses to send can hand back the state the caller already had instead of
 * going to the network for it.
 */
const lastSeen = new Map<string, UserPreferences>()

export async function get_user_preferences(userId: string): Promise<UserPreferences> {
	const preferences = await invoke<UserPreferences>('plugin:users|get_user_preferences', {
		userId,
	})
	lastSeen.set(userId, preferences)
	return preferences
}

/**
 * Never writes. The launcher keeps its settings on this device only.
 *
 * This fork changes settings Modrinth's server does not know about — its own
 * themes, its own feature set — so a write is rejected, and the user is told
 * their settings could not be saved for a change that was never theirs to sync.
 * The read side is left alone: the pages that show account preferences still
 * work.
 *
 * The guard lives here rather than at the two call sites because this is the
 * single place the app's writes pass through, and because the settings pages
 * are exactly where upstream edits land — an earlier attempt that deleted the
 * calls there was undone by a merge that rewrote the same functions.
 *
 * `preferences` is deliberately ignored, and the account's current preferences
 * are returned unchanged so the caller's cache stays truthful. A read that
 * fails falls back to the last value seen, rather than becoming an error: the
 * write did not happen, so "could not save" would be the wrong thing to say.
 */
export async function patch_user_preferences(
	userId: string,
	preferences: PartialUserPreferences,
): Promise<UserPreferences> {
	void preferences

	try {
		return await get_user_preferences(userId)
	} catch (error) {
		const previous = lastSeen.get(userId)
		if (previous) {
			return previous
		}
		throw error
	}
}