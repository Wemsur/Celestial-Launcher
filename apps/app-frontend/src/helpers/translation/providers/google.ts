// Google's `gtx` endpoint.
//
// Kept as a fallback for users outside mainland China — Google Translate has
// been unreachable from the mainland since 2022, so this must never be the
// default.
//
// Uses `translate_a/t`, which takes a repeated `q` parameter, instead of
// `translate_a/single`, which takes one. The endpoint throttles per IP, and a
// project description is dozens of separate text nodes, so one request per node
// runs into 429 almost immediately.
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'

import {
	BROWSER_USER_AGENT,
	byteLength,
	describeFailure,
	regionTarget,
	splitByBytes,
	TransientTranslationError,
	type TranslationProvider,
	withRetry,
} from '../shared'

const TRANSLATE_URL = 'https://translate.googleapis.com/translate_a/t'
const MAX_ITEMS_PER_REQUEST = 16
/** Everything travels in the query string, so this stays well below the URL limit. */
const MAX_BYTES_PER_REQUEST = 1600

async function post(texts: string[], target: string): Promise<string[]> {
	const params = new URLSearchParams({
		client: 'gtx',
		sl: 'auto',
		tl: target,
		dt: 't',
		ie: 'UTF-8',
		oe: 'UTF-8',
	})
	for (const text of texts) params.append('q', text)

	const response = await tauriFetch(`${TRANSLATE_URL}?${params.toString()}`, {
		method: 'GET',
		headers: {
			Accept: '*/*',
			'User-Agent': BROWSER_USER_AGENT,
		},
	})

	if (!response.ok) throw await describeFailure(response, 'Google Translate failed')

	const payload = (await response.json()) as unknown
	const results = extract(payload, texts.length)
	if (!results) {
		throw new Error(
			`Google Translate returned an unexpected payload: ${JSON.stringify(payload).slice(0, 180)}`,
		)
	}

	return results
}

/**
 * `translate_a/t` answers in one of several shapes depending on how many texts
 * were sent and whether it felt like including the detected language:
 * `["translated"]`, `["translated","en"]`, `[["a"],["b"]]`, `[["a","en"],["b","en"]]`.
 */
function extract(payload: unknown, count: number): string[] | null {
	if (!Array.isArray(payload)) return null

	const flat = payload.map((entry) => {
		if (typeof entry === 'string') return entry
		if (Array.isArray(entry) && typeof entry[0] === 'string') return entry[0]
		return null
	})

	if (flat.length === count && !flat.includes(null)) return flat as string[]

	// A single text with its detected language appended.
	if (count === 1 && typeof payload[0] === 'string') return [payload[0]]

	return null
}

/** Groups the batch into requests that respect both limits. */
function chunkBatch(texts: string[]): string[][] {
	const groups: string[][] = []
	let current: string[] = []
	let bytes = 0

	for (const text of texts) {
		const size = byteLength(text)
		const full = current.length >= MAX_ITEMS_PER_REQUEST
		if (current.length > 0 && (full || bytes + size > MAX_BYTES_PER_REQUEST)) {
			groups.push(current)
			current = []
			bytes = 0
		}
		current.push(text)
		bytes += size
	}

	if (current.length > 0) groups.push(current)
	return groups
}

export const googleProvider: TranslationProvider = {
	id: 'google',
	label: 'Google Translate',
	maxItems: 48,
	maxChars: 8000,
	targetLang: regionTarget,
	async translate(texts, target) {
		// A text over the per-request limit becomes several requests of its own.
		const prepared = texts.map((text) => splitByBytes(text, MAX_BYTES_PER_REQUEST))
		const flat = prepared.flat()

		const translated: string[] = []
		for (const group of chunkBatch(flat)) {
			translated.push(...(await withRetry(() => post(group, target))))
		}

		if (translated.length !== flat.length) {
			throw new TransientTranslationError('Google Translate returned an incomplete batch')
		}

		// Stitch the split pieces back onto their original text.
		let cursor = 0
		return prepared.map((chunks) => {
			const parts = translated.slice(cursor, cursor + chunks.length)
			cursor += chunks.length
			return parts.join(' ')
		})
	},
}
