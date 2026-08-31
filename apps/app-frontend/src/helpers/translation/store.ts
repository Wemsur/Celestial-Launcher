// Disk persistence for the content translation feature.
//
// Both files live in their own folder, `<appdata>/translation/`:
//   settings.json  the chosen translation service
//   cache.json     translated strings, kept for 7 days
//
// The Rust side (see `apps/app/src/main.rs`) only reads and writes the raw
// text, so the schema below is owned entirely by the frontend.
import { invoke } from '@tauri-apps/api/core'

import type { TranslationServiceId } from './services'

/** How long a cached translation stays usable. */
export const TRANSLATION_CACHE_TTL_MS = 7 * 24 * 60 * 60 * 1000
/** Hard cap so a long browsing session cannot grow the file without bound. */
export const TRANSLATION_CACHE_MAX_ENTRIES = 8000

export interface TranslationSettings {
	service: TranslationServiceId
}

export interface TranslationCacheEntry {
	/** Translated text. */
	t: string
	/** Epoch ms of the last time this entry was written or used. */
	at: number
}

export interface TranslationCacheFile {
	version: 1
	entries: Record<string, TranslationCacheEntry>
}

export async function loadTranslationSettings(): Promise<Partial<TranslationSettings>> {
	try {
		const raw = await invoke<string>('load_translation_settings')
		if (!raw) return {}
		const parsed = JSON.parse(raw)
		return typeof parsed === 'object' && parsed !== null ? parsed : {}
	} catch (error) {
		console.error('Failed to load translation settings:', error)
		return {}
	}
}

export async function saveTranslationSettings(settings: TranslationSettings): Promise<void> {
	try {
		await invoke('save_translation_settings', { contents: JSON.stringify(settings, null, 2) })
	} catch (error) {
		console.error('Failed to save translation settings:', error)
	}
}

/** Reads the cache file and drops everything older than the TTL. */
export async function loadTranslationCache(): Promise<Map<string, TranslationCacheEntry>> {
	const entries = new Map<string, TranslationCacheEntry>()

	try {
		const raw = await invoke<string>('load_translation_cache')
		if (!raw) return entries

		const parsed = JSON.parse(raw) as TranslationCacheFile
		if (!parsed || typeof parsed.entries !== 'object' || parsed.entries === null) return entries

		const cutoff = Date.now() - TRANSLATION_CACHE_TTL_MS
		for (const [key, entry] of Object.entries(parsed.entries)) {
			if (typeof entry?.t !== 'string' || typeof entry?.at !== 'number') continue
			if (entry.at < cutoff) continue
			entries.set(key, entry)
		}
	} catch (error) {
		console.error('Failed to load translation cache:', error)
	}

	return entries
}

/** Writes the cache back, dropping expired entries and the oldest overflow. */
export async function saveTranslationCache(
	cache: Map<string, TranslationCacheEntry>,
): Promise<void> {
	const cutoff = Date.now() - TRANSLATION_CACHE_TTL_MS
	let kept = [...cache.entries()].filter(([, entry]) => entry.at >= cutoff)

	if (kept.length > TRANSLATION_CACHE_MAX_ENTRIES) {
		kept.sort((a, b) => b[1].at - a[1].at)
		kept = kept.slice(0, TRANSLATION_CACHE_MAX_ENTRIES)
	}

	const file: TranslationCacheFile = { version: 1, entries: Object.fromEntries(kept) }

	try {
		await invoke('save_translation_cache', { contents: JSON.stringify(file) })
	} catch (error) {
		console.error('Failed to save translation cache:', error)
	}
}

export async function clearTranslationCache(): Promise<void> {
	try {
		await invoke('clear_translation_cache')
	} catch (error) {
		console.error('Failed to clear translation cache:', error)
	}
}
