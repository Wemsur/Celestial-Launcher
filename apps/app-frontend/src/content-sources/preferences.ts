/**
 * The user's content-source preferences: which sources Discover pulls from, and
 * each source's download-count multiplier.
 *
 * Selection is stored as the set of *excluded* sources rather than the included
 * ones, so that "everything on" is the natural default and a newly installed
 * source is picked up without the user touching anything. Multipliers exist so a
 * source whose download counts are on a different scale (a small site next to
 * Modrinth) does not dominate the downloads sort; each is applied to that
 * source's counts before the merged list is ordered.
 */

import { shallowRef } from 'vue'

/** The built-in Modrinth source, which is not a plugin but still selectable. */
export const MODRINTH_SOURCE_ID = 'modrinth'

export const DEFAULT_MULTIPLIER = 1
export const MIN_MULTIPLIER = 0.01
export const MAX_MULTIPLIER = 10_000_000

const STORAGE_KEY = 'celestial-content-source-preferences'

interface StoredPreferences {
	excluded: string[]
	multipliers: Record<string, number>
}

function load(): StoredPreferences {
	if (typeof localStorage === 'undefined') {
		return { excluded: [], multipliers: {} }
	}
	try {
		const raw = localStorage.getItem(STORAGE_KEY)
		if (!raw) return { excluded: [], multipliers: {} }
		const parsed = JSON.parse(raw) as Partial<StoredPreferences>
		return {
			excluded: Array.isArray(parsed.excluded)
				? parsed.excluded.filter((id): id is string => typeof id === 'string')
				: [],
			multipliers:
				parsed.multipliers && typeof parsed.multipliers === 'object'
					? Object.fromEntries(
							Object.entries(parsed.multipliers).filter(
								([, value]) => typeof value === 'number' && Number.isFinite(value),
							),
						)
					: {},
		}
	} catch {
		return { excluded: [], multipliers: {} }
	}
}

const preferences = shallowRef<StoredPreferences>(load())

function commit(next: StoredPreferences): void {
	preferences.value = next
	if (typeof localStorage !== 'undefined') {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
	}
}

/** The live preferences. Reactive. */
export const sourcePreferences = preferences

/** Whether a source contributes to Discover. Everything is on by default. */
export function isSourceActive(sourceId: string): boolean {
	return !preferences.value.excluded.includes(sourceId)
}

export function setSourceActive(sourceId: string, active: boolean): void {
	const excluded = new Set(preferences.value.excluded)
	if (active) {
		excluded.delete(sourceId)
	} else {
		excluded.add(sourceId)
	}
	commit({ ...preferences.value, excluded: [...excluded] })
}

/** Clamp a multiplier into the allowed range; non-numbers fall back to 1. */
export function normalizeMultiplier(value: number): number {
	if (!Number.isFinite(value)) return DEFAULT_MULTIPLIER
	return Math.min(MAX_MULTIPLIER, Math.max(MIN_MULTIPLIER, value))
}

export function sourceMultiplier(sourceId: string): number {
	return preferences.value.multipliers[sourceId] ?? DEFAULT_MULTIPLIER
}

export function setSourceMultiplier(sourceId: string, value: number): void {
	commit({
		...preferences.value,
		multipliers: {
			...preferences.value.multipliers,
			[sourceId]: normalizeMultiplier(value),
		},
	})
}

export function resetSourceMultipliers(): void {
	commit({ ...preferences.value, multipliers: {} })
}
