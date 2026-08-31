// The preset translation services.
//
// Only keyless services are offered, and only as fixed presets: a user-supplied
// endpoint would mean widening the Tauri HTTP allowlist in
// `apps/app/capabilities/plugins.json` to arbitrary domains.
import { googleProvider } from './providers/google'
import { microsoftProvider } from './providers/microsoft'
import { myMemoryProvider } from './providers/mymemory'
import { transmartProvider } from './providers/transmart'
import type { TranslationProvider } from './shared'

export type { TranslationProvider } from './shared'

export const TRANSLATION_PROVIDERS = [
	transmartProvider,
	microsoftProvider,
	googleProvider,
	myMemoryProvider,
] as const

// Declared rather than derived: the provider objects are typed as
// `TranslationProvider`, so their `id` widens to `string`.
export type TranslationServiceId = 'microsoft' | 'transmart' | 'google' | 'mymemory'

/**
 * Takes 30 texts in a single request, needs no token handshake at all, and is
 * the one option reachable from mainland China without a proxy — so a fresh
 * install starts here. Only affects first run; the choice is persisted.
 */
export const DEFAULT_TRANSLATION_SERVICE: TranslationServiceId = 'transmart'

export function getTranslationProvider(id: string): TranslationProvider {
	return (
		TRANSLATION_PROVIDERS.find((provider) => provider.id === id) ??
		TRANSLATION_PROVIDERS.find((provider) => provider.id === DEFAULT_TRANSLATION_SERVICE)!
	)
}

export function isTranslationServiceId(id: unknown): id is TranslationServiceId {
	return typeof id === 'string' && TRANSLATION_PROVIDERS.some((provider) => provider.id === id)
}
