// Shared plumbing for the translation providers.
//
// Every provider here is a keyless endpoint used by a browser or a web UI, so
// requests go through `@tauri-apps/plugin-http` (Rust side, no CORS check) and
// carry a browser User-Agent. Any of them can start refusing traffic at any
// time, which is why each one is a separate, replaceable adapter.

/** Chromium UA. The keyless endpoints reject obviously non-browser clients. */
export const BROWSER_USER_AGENT =
	'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0'

export interface TranslationProvider {
	id: string
	/** Brand name; shown as-is in settings, never translated. */
	label: string
	/** Most texts the provider accepts in one request. */
	maxItems: number
	/** Most characters (summed over the batch) to put in one request. */
	maxChars: number
	/** Maps a launcher locale such as `zh-CN` to the provider's own code. */
	targetLang: (locale: string) => string
	/** Returns exactly one translation per input, in the same order. */
	translate: (texts: string[], target: string) => Promise<string[]>
}

/**
 * A refusal we expect to go away on its own: rate limits and server hiccups.
 * The queue re-tries these later instead of writing the text off for the
 * session, which matters because the free endpoints throttle by IP.
 */
export class TransientTranslationError extends Error {
	constructor(message: string) {
		super(message)
		this.name = 'TransientTranslationError'
	}
}

export function isTransientTranslationError(error: unknown): boolean {
	return error instanceof TransientTranslationError
}

/** Statuses that mean "not now" rather than "not ever". */
export function isTransientStatus(status: number): boolean {
	return status === 408 || status === 425 || status === 429 || status >= 500
}

export function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * Builds an error message that includes what the server actually said. These
 * endpoints are undocumented, so the response body is the only way to tell a
 * changed request shape from a block.
 */
export async function describeFailure(response: Response, what: string): Promise<Error> {
	let detail = ''
	try {
		detail = (await response.text()).trim().replace(/\s+/g, ' ').slice(0, 180)
	} catch {
		// The body is a nicety; the status is the part that always exists.
	}

	const message = detail ? `${what} (${response.status}): ${detail}` : `${what} (${response.status})`
	return isTransientStatus(response.status)
		? new TransientTranslationError(message)
		: new Error(message)
}

/**
 * Retries only transient failures, with a short backoff. Kept small on purpose:
 * a translation is never worth hammering a free service for.
 */
export async function withRetry<T>(run: () => Promise<T>, delays = [900, 2600]): Promise<T> {
	let lastError: unknown

	for (let attempt = 0; ; attempt++) {
		try {
			return await run()
		} catch (error) {
			lastError = error
			if (attempt >= delays.length || !isTransientTranslationError(error)) break
			await sleep(delays[attempt])
		}
	}

	throw lastError
}

export function primarySubtag(locale: string): string {
	return (locale.split('-')[0] || 'en').toLowerCase()
}

/**
 * Chinese is the only locale the launcher ships where the primary subtag is
 * not enough — everything else (de, fr, ja, ru, ...) works as-is.
 */
export function isTraditionalChinese(locale: string): boolean {
	const lower = locale.toLowerCase()
	return lower.startsWith('zh-tw') || lower.startsWith('zh-hk') || lower.includes('hant')
}

/** BCP-47 style codes, as used by Microsoft. */
export function bcp47Target(locale: string): string {
	const lower = locale.toLowerCase()
	if (lower.startsWith('zh')) return isTraditionalChinese(locale) ? 'zh-Hant' : 'zh-Hans'
	if (lower === 'pt-pt') return 'pt-PT'
	return primarySubtag(locale)
}

/** Region codes, as used by Google and MyMemory. */
export function regionTarget(locale: string): string {
	if (locale.toLowerCase().startsWith('zh')) {
		return isTraditionalChinese(locale) ? 'zh-TW' : 'zh-CN'
	}
	return primarySubtag(locale)
}

const encoder = new TextEncoder()

export function byteLength(text: string): number {
	return encoder.encode(text).length
}

/**
 * Splits text that is too long for a provider's per-request limit, preferring
 * sentence then word boundaries so the machine translation still sees whole
 * clauses. Rejoined with a space by the caller.
 */
export function splitByBytes(text: string, maxBytes: number): string[] {
	if (byteLength(text) <= maxBytes) return [text]

	const chunks: string[] = []
	let current = ''

	const flush = () => {
		if (current) chunks.push(current)
		current = ''
	}

	// Keep the delimiter attached to the piece it terminates.
	for (const piece of text.split(/(?<=[.!?。！？\n])\s+/)) {
		if (!piece) continue

		if (byteLength(piece) > maxBytes) {
			flush()
			for (const word of piece.split(/\s+/)) {
				const candidate = current ? `${current} ${word}` : word
				if (byteLength(candidate) > maxBytes) {
					flush()
					// A single "word" over the limit only happens with CJK runs.
					current = byteLength(word) > maxBytes ? hardCut(word, maxBytes, chunks) : word
				} else {
					current = candidate
				}
			}
			continue
		}

		const candidate = current ? `${current} ${piece}` : piece
		if (byteLength(candidate) > maxBytes) {
			flush()
			current = piece
		} else {
			current = candidate
		}
	}

	flush()
	return chunks
}

/** Last resort for an unbroken run longer than the limit. Returns the tail. */
function hardCut(word: string, maxBytes: number, chunks: string[]): string {
	let current = ''
	for (const char of word) {
		if (byteLength(current + char) > maxBytes) {
			chunks.push(current)
			current = char
		} else {
			current += char
		}
	}
	return current
}
