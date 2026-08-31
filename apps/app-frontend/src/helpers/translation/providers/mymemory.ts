// MyMemory.
//
// The only provider here with a documented, officially free keyless tier — and
// the most limited: 500 bytes per request, no batching, and a small daily quota
// per IP. Kept as a last-resort fallback.
//
// It also needs an explicit source language, so English is assumed: Modrinth
// project descriptions are written in English in all but a few cases.
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'

import {
	type Attempted,
	BROWSER_USER_AGENT,
	describeFailure,
	mapWithPool,
	regionTarget,
	settle,
	splitByBytes,
	TransientTranslationError,
	type TranslationProvider,
	withRetry,
} from '../shared'

const TRANSLATE_URL = 'https://api.mymemory.translated.net/get'
const SOURCE_LANG = 'en'
/** The documented cap is 500 bytes; leave room for the delimiter handling. */
const MAX_BYTES_PER_REQUEST = 450
/** Requests in flight. Its free quota is small, so this stays low. */
const CONCURRENCY = 2

interface MyMemoryResponse {
	responseData?: { translatedText?: string }
	responseStatus?: number | string
	responseDetails?: string
}

/** Quota and length problems come back as a 200 with an error in the text. */
function isErrorText(text: string): boolean {
	const upper = text.toUpperCase()
	return (
		upper.includes('MYMEMORY WARNING') ||
		upper.includes('QUERY LENGTH LIMIT EXCEEDED') ||
		upper.includes('INVALID LANGUAGE PAIR')
	)
}

async function translateOne(text: string, target: string): Promise<string> {
	const params = new URLSearchParams({
		q: text,
		langpair: `${SOURCE_LANG}|${target}`,
		mt: '1',
	})

	const response = await tauriFetch(`${TRANSLATE_URL}?${params.toString()}`, {
		method: 'GET',
		headers: { 'User-Agent': BROWSER_USER_AGENT },
	})

	if (!response.ok) throw await describeFailure(response, 'MyMemory failed')

	const payload = (await response.json()) as MyMemoryResponse
	const status = Number(payload?.responseStatus)
	const details = payload?.responseDetails ?? ''
	if (status !== 200) {
		const message = `MyMemory failed (${details || status})`
		// The daily-quota notice is the usual one, and it lifts on its own.
		throw /limit|quota|too many|later/i.test(details)
			? new TransientTranslationError(message)
			: new Error(message)
	}

	const translated = payload?.responseData?.translatedText
	if (typeof translated !== 'string' || isErrorText(translated)) {
		const message = `MyMemory refused the request (${translated ?? 'no text'})`
		throw /limit|quota/i.test(translated ?? '')
			? new TransientTranslationError(message)
			: new Error(message)
	}

	return translated
}

export const myMemoryProvider: TranslationProvider = {
	id: 'mymemory',
	label: 'MyMemory',
	// One request per text (or per 450-byte piece of it), so batches stay small.
	maxItems: 4,
	maxChars: 1800,
	targetLang: regionTarget,
	async translate(texts, target) {
		const results = await mapWithPool<string, Attempted>(texts, CONCURRENCY, async (text) => {
			try {
				const translated: string[] = []
				for (const chunk of splitByBytes(text, MAX_BYTES_PER_REQUEST)) {
					translated.push(await withRetry(() => translateOne(chunk, target)))
				}
				return { text: translated.join(' ') }
			} catch (error) {
				// One refused text must not throw away the ones that worked.
				return { error }
			}
		})

		return settle(results)
	},
}
