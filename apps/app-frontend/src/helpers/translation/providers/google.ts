// Google's keyless endpoints.
//
// Kept as a fallback for users outside mainland China — Google Translate has
// been unreachable from the mainland since 2022, so this must never be the
// default. Nothing here is documented, so three routes are tried in order and
// the session sticks to the first one that actually works:
//
//  1. `clients5.google.com/translate_a/t` with Chrome's dictionary client id.
//     Batches, and by far the most tolerant of repeated requests.
//  2. `translate.googleapis.com/translate_a/t` with the `gtx` client. Batches,
//     but answers 200 with the text unchanged when it does not feel like it.
//  3. `translate.googleapis.com/translate_a/single`, one text per request. The
//     slowest and the easiest to get throttled on, hence last.
//
// A 429 here often means Google served its "Sorry..." interstitial, i.e. it is
// refusing the whole network. That is reported as a hard failure on purpose:
// retrying an anti-abuse block only makes it last longer.
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'

import {
	type Attempted,
	BROWSER_USER_AGENT,
	byteLength,
	describeFailure,
	isTransientTranslationError,
	looksLikeHtml,
	mapWithPool,
	readBody,
	regionTarget,
	settle,
	splitByBytes,
	type TranslationProvider,
	withRetry,
} from '../shared'

const CHROME_DICT_URL = 'https://clients5.google.com/translate_a/t'
const BATCH_URL = 'https://translate.googleapis.com/translate_a/t'
const SINGLE_URL = 'https://translate.googleapis.com/translate_a/single'

const MAX_ITEMS_PER_REQUEST = 16
/** A batch travels in the query string, so this stays below the URL limit. */
const MAX_BYTES_PER_REQUEST = 1600
/** Requests in flight in single-text mode. Google throttles hard per IP. */
const SINGLE_CONCURRENCY = 1
/** Longer than the shared default: a throttled Google needs real seconds. */
const RETRY_DELAYS = [2000, 6000]

const ROUTES = ['chrome-dict', 'gtx-batch', 'gtx-single'] as const
type Route = (typeof ROUTES)[number]

let route: Route = 'chrome-dict'

function nextRoute(current: Route): Route | null {
	return ROUTES[ROUTES.indexOf(current) + 1] ?? null
}

function headers(): Record<string, string> {
	return { Accept: '*/*', 'User-Agent': BROWSER_USER_AGENT }
}

/**
 * Google is refusing the whole network, not this particular request. Marked so
 * the route list is left alone: every route goes to the same Google, and the
 * block expires on its own, so the working route must not be discarded.
 */
class BlockedError extends Error {}

/** Google's anti-abuse page, as opposed to an ordinary rate-limit reply. */
async function failure(response: Response, what: string): Promise<Error> {
	const body = await readBody(response)
	if (looksLikeHtml(body)) {
		return new BlockedError(
			`${what}: Google is refusing traffic from this network (${response.status} anti-abuse page). Pick another service in Settings.`,
		)
	}

	return describeFailure(response, what, body)
}

async function postBatch(current: Route, texts: string[], target: string): Promise<string[]> {
	const dictionary = current === 'chrome-dict'
	const params = new URLSearchParams({
		client: dictionary ? 'dict-chrome-ex' : 'gtx',
		sl: 'auto',
		tl: target,
		dt: 't',
	})
	for (const text of texts) params.append('q', text)

	const url = `${dictionary ? CHROME_DICT_URL : BATCH_URL}?${params.toString()}`
	const response = await tauriFetch(url, { method: 'GET', headers: headers() })

	if (!response.ok) throw await failure(response, 'Google Translate failed')

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
 * These endpoints answer in one of several shapes depending on how many texts
 * were sent and whether they felt like including the detected language:
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

/**
 * True when the reply is just the input back. A real translation can match its
 * source (`Fabric API`), so this only counts as an echo when nothing at all
 * changed across enough text to be sure.
 */
function isEcho(texts: string[], results: string[]): boolean {
	const total = texts.reduce((sum, text) => sum + text.length, 0)
	if (total < 24) return false

	return results.every((result, index) => result.trim() === texts[index].trim())
}

/**
 * Guards against a route answering in a shape this code reads wrongly — a
 * language tag where a translation should be, for instance. Chinese is compact
 * but never eight characters for a whole paragraph.
 */
function looksTruncated(texts: string[], results: string[]): boolean {
	return results.some((result, index) => {
		const source = texts[index]
		return source.length > 40 && result.length <= 8 && result.length * 8 < source.length
	})
}

/** Internal marker: the route answered, but not with usable translations. */
class UnusableRouteError extends Error {}

async function translateOne(text: string, target: string): Promise<string> {
	const url = `${SINGLE_URL}?client=gtx&sl=auto&dt=t&tl=${encodeURIComponent(target)}`

	const response = await tauriFetch(url, {
		method: 'POST',
		headers: {
			...headers(),
			'Content-Type': 'application/x-www-form-urlencoded;charset=UTF-8',
		},
		body: `q=${encodeURIComponent(text)}`,
	})

	if (!response.ok) throw await failure(response, 'Google Translate failed')

	// Shape: [[[translated, original, ...], ...], ...] — one entry per sentence.
	const payload = (await response.json()) as unknown
	const segments = Array.isArray(payload) ? payload[0] : null
	if (!Array.isArray(segments)) {
		throw new Error(
			`Google Translate returned an unexpected payload: ${JSON.stringify(payload).slice(0, 180)}`,
		)
	}

	return segments
		.map((segment) => (Array.isArray(segment) ? segment[0] : null))
		.filter((part): part is string => typeof part === 'string')
		.join('')
}

/** Groups a batch into requests that respect both limits. */
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

async function translateBatched(
	current: Route,
	texts: string[],
	target: string,
): Promise<string[]> {
	const translated: string[] = []

	for (const group of chunkBatch(texts)) {
		const results = await withRetry(() => postBatch(current, group, target), RETRY_DELAYS)
		if (isEcho(group, results) || looksTruncated(group, results)) {
			throw new UnusableRouteError('Google Translate echoed the request instead of translating it')
		}
		translated.push(...results)
	}

	return translated
}

async function translateIndividually(texts: string[], target: string): Promise<(string | null)[]> {
	const results = await mapWithPool<string, Attempted>(texts, SINGLE_CONCURRENCY, async (text) => {
		try {
			return { text: await withRetry(() => translateOne(text, target), RETRY_DELAYS) }
		} catch (error) {
			// One refused text must not throw away the ones that worked.
			return { error }
		}
	})

	return settle(results)
}

function runRoute(current: Route, texts: string[], target: string): Promise<(string | null)[]> {
	return current === 'gtx-single'
		? translateIndividually(texts, target)
		: translateBatched(current, texts, target)
}

export const googleProvider: TranslationProvider = {
	id: 'google',
	label: 'Google Translate',
	maxItems: 16,
	maxChars: 4000,
	targetLang: regionTarget,
	async translate(texts, target) {
		// A text over the per-request limit becomes several pieces of its own.
		const prepared = texts.map((text) => splitByBytes(text, MAX_BYTES_PER_REQUEST))
		const flat = prepared.flat()

		let attempt: (string | null)[] | null = null

		while (!attempt) {
			const current = route

			try {
				attempt = await runRoute(current, flat, target)
			} catch (error) {
				// Throttling says "later", not "use another route", and a network-wide
				// block is not the route's fault either.
				if (isTransientTranslationError(error) || error instanceof BlockedError) throw error

				const fallback = nextRoute(current)
				if (!fallback) throw error

				// Only move on if nobody else already did.
				if (route === current) route = fallback
				console.warn(`Google Translate route "${current}" unusable, trying "${fallback}":`, error)
			}
		}

		// A `const` so the closure below keeps the narrowed type.
		const translated = attempt

		// Stitch the split pieces back onto their original text; a piece that
		// failed drops the whole text, since half a translation is worse than none.
		let cursor = 0
		return prepared.map((chunks) => {
			const parts = translated.slice(cursor, cursor + chunks.length)
			cursor += chunks.length
			return parts.includes(null) ? null : parts.join(' ')
		})
	},
}
