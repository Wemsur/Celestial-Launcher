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
	BROWSER_USER_AGENT,
	describeFailure,
	regionTarget,
	splitByBytes,
	TransientTranslationError,
	type TranslationProvider,
	withRetry,
} from '../shared'

const TRANSLATE_URL = 'https://api.mymemory.translated.net/get'
const SOURCE_LANG = 'en'
/** The documented cap is 500 bytes; leave room for the delimiter handling. */
const MAX_BYTES_PER_REQUEST = 450

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
	maxItems: 1,
	maxChars: MAX_BYTES_PER_REQUEST,
	targetLang: regionTarget,
	async translate(texts, target) {
		const results: string[] = []

		for (const text of texts) {
			const translated: string[] = []
			for (const chunk of splitByBytes(text, MAX_BYTES_PER_REQUEST)) {
				translated.push(await withRetry(() => translateOne(chunk, target)))
			}
			results.push(translated.join(' '))
		}

		return results
	},
}
