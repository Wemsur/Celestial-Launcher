// Microsoft's keyless translators.
//
// Two routes to the same engine, tried in that order:
//
//  1. Edge's built-in translator — `edge.microsoft.com/translate/auth` hands out
//     a short-lived JWT for the Translator v3 API, which takes a whole batch of
//     texts in one request. Cheapest by far when it works.
//  2. The Bing Translator web page — scrape the anti-abuse token out of the HTML
//     and post to `ttranslatev3`, one text per request. Slower, but it is the
//     route the public web UI itself uses, so it stays alive when the Edge auth
//     endpoint moves. Also reachable from mainland China (via cn.bing.com).
//
// The first hard failure of route 1 switches this session to route 2 so we do
// not pay for a doomed auth request on every batch.
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'

import {
	type Attempted,
	bcp47Target,
	BROWSER_USER_AGENT,
	describeFailure,
	mapWithPool,
	settle,
	TransientTranslationError,
	type TranslationProvider,
	withRetry,
} from '../shared'

const AUTH_URL = 'https://edge.microsoft.com/translate/auth'
const TRANSLATE_URL = 'https://api-edge.cognitive.microsofttranslator.com/translate'
const BING_PAGE_URL = 'https://www.bing.com/translator'
const BING_TRANSLATE_URL = 'https://www.bing.com/ttranslatev3'

/** The Edge token is good for about ten minutes; refresh a little early. */
const TOKEN_TTL_MS = 9 * 60 * 1000
/** The Bing page token lives roughly an hour. */
const BING_SESSION_TTL_MS = 25 * 60 * 1000
/** Bing takes one text per request, so a few go out at once. */
const BING_CONCURRENCY = 3

let token: { value: string; expiresAt: number } | null = null
let tokenPromise: Promise<string> | null = null
let edgeAuthBroken = false
let bingSession: BingSession | null = null
let bingSessionPromise: Promise<BingSession> | null = null

/** Shared between workers: several batches must not each fetch a token. */
function getToken(force = false): Promise<string> {
	if (!force && token && token.expiresAt > Date.now()) return Promise.resolve(token.value)

	tokenPromise ??= fetchToken().finally(() => {
		tokenPromise = null
	})

	return tokenPromise
}

async function fetchToken(): Promise<string> {
	const response = await tauriFetch(AUTH_URL, {
		method: 'GET',
		headers: {
			Accept: '*/*',
			'Accept-Language': 'en-US,en;q=0.9',
			'User-Agent': BROWSER_USER_AGENT,
		},
	})
	if (!response.ok) throw await describeFailure(response, 'Edge translator auth failed')

	const value = (await response.text()).trim()
	if (!value) throw new Error('Edge translator returned an empty token')

	token = { value, expiresAt: Date.now() + TOKEN_TTL_MS }
	return value
}

type MetResponse = { translations?: { text?: string }[] }[]

async function metPost(texts: string[], target: string, authToken: string): Promise<Response> {
	const url = `${TRANSLATE_URL}?api-version=3.0&to=${encodeURIComponent(target)}&textType=plain`

	return tauriFetch(url, {
		method: 'POST',
		headers: {
			'Content-Type': 'application/json',
			'User-Agent': BROWSER_USER_AGENT,
			Authorization: `Bearer ${authToken}`,
		},
		body: JSON.stringify(texts.map((text) => ({ Text: text }))),
	})
}

async function translateViaEdge(texts: string[], target: string): Promise<string[]> {
	let response = await metPost(texts, target, await getToken())

	// An expired token is the one failure worth retrying automatically.
	if (response.status === 401 || response.status === 403) {
		response = await metPost(texts, target, await getToken(true))
	}

	if (!response.ok) throw await describeFailure(response, 'Edge translator failed')

	const payload = (await response.json()) as MetResponse
	if (!Array.isArray(payload) || payload.length !== texts.length) {
		throw new Error(
			`Edge translator returned an unexpected payload: ${JSON.stringify(payload).slice(0, 180)}`,
		)
	}

	return payload.map((item, index) => item?.translations?.[0]?.text ?? texts[index])
}

interface BingSession {
	ig: string
	iid: string
	key: string
	token: string
	expiresAt: number
}

/** Shared between workers: six parallel texts must not scrape the page six times. */
function getBingSession(force = false): Promise<BingSession> {
	if (!force && bingSession && bingSession.expiresAt > Date.now()) {
		return Promise.resolve(bingSession)
	}

	bingSessionPromise ??= fetchBingSession().finally(() => {
		bingSessionPromise = null
	})

	return bingSessionPromise
}

async function fetchBingSession(): Promise<BingSession> {
	const response = await tauriFetch(BING_PAGE_URL, {
		method: 'GET',
		headers: {
			Accept: 'text/html,application/xhtml+xml',
			'Accept-Language': 'en-US,en;q=0.9',
			'User-Agent': BROWSER_USER_AGENT,
		},
	})
	if (!response.ok) throw await describeFailure(response, 'Bing translator page failed')

	const html = await response.text()

	// `var params_AbusePreventionHelper = [<key>,"<token>",<ttl>];`
	const helper = /params_AbusePreventionHelper\s*=\s*\[([^\]]+)\]/.exec(html)?.[1]?.split(',')
	const session: BingSession = {
		ig: /IG:"([^"]+)"/.exec(html)?.[1] ?? '',
		iid: /data-iid="([^"]+)"/.exec(html)?.[1] ?? 'translator.5024',
		key: helper?.[0]?.trim() ?? '',
		token: helper?.[1]?.trim().replace(/^"|"$/g, '') ?? '',
		expiresAt: Date.now() + BING_SESSION_TTL_MS,
	}

	if (!session.ig || !session.key || !session.token) {
		throw new Error('Bing translator page did not contain a usable session token')
	}

	bingSession = session
	return session
}

type BingResponse =
	| { translations?: { text?: string }[] }[]
	/** Bing reports its own errors with HTTP 200 and this shape. */
	| { statusCode?: number; errorMessage?: string }

async function bingPost(text: string, target: string, session: BingSession): Promise<Response> {
	const url = `${BING_TRANSLATE_URL}?isVertical=1&&IG=${session.ig}&IID=${session.iid}`
	const body = new URLSearchParams({
		fromLang: 'auto-detect',
		text,
		to: target,
		token: session.token,
		key: session.key,
	})

	return tauriFetch(url, {
		method: 'POST',
		headers: {
			'Content-Type': 'application/x-www-form-urlencoded',
			'User-Agent': BROWSER_USER_AGENT,
			Referer: BING_PAGE_URL,
			Origin: 'https://www.bing.com',
		},
		body: body.toString(),
	})
}

async function translateOneViaBing(text: string, target: string): Promise<string> {
	// Two passes: a token that went stale mid-session is worth one refresh.
	for (let attempt = 0; attempt < 2; attempt++) {
		const response = await bingPost(text, target, await getBingSession(attempt > 0))

		if (response.status === 400 || response.status === 401) continue
		if (!response.ok) throw await describeFailure(response, 'Bing translator failed')

		const payload = (await response.json()) as BingResponse

		if (Array.isArray(payload)) {
			const translated = payload[0]?.translations?.[0]?.text
			if (typeof translated === 'string') return translated

			throw new Error(
				`Bing translator returned an unexpected payload: ${JSON.stringify(payload).slice(0, 180)}`,
			)
		}

		// Bing reports a rejected token as HTTP 200 with the status in the body.
		if (payload?.statusCode === 400 || payload?.statusCode === 401) continue

		const reason = payload?.errorMessage ?? JSON.stringify(payload).slice(0, 180)
		throw new Error(`Bing translator refused the request: ${reason}`)
	}

	throw new TransientTranslationError('Bing translator rejected its own session token twice')
}

async function translateViaBing(texts: string[], target: string): Promise<(string | null)[]> {
	const results = await mapWithPool<string, Attempted>(texts, BING_CONCURRENCY, async (text) => {
		try {
			return { text: await withRetry(() => translateOneViaBing(text, target)) }
		} catch (error) {
			// One bad text must not throw away the ones that worked.
			return { error }
		}
	})

	return settle(results)
}

export const microsoftProvider: TranslationProvider = {
	id: 'microsoft',
	label: 'Microsoft (Edge / Bing)',
	// Sized for the Bing fallback: a batch this big is a single Edge request but
	// a handful of Bing round trips, and anything larger feels stuck.
	maxItems: 12,
	maxChars: 3000,
	targetLang: bcp47Target,
	async translate(texts, target) {
		if (!edgeAuthBroken) {
			try {
				// One attempt only: route 1 is a bonus, and making the first batch of a
				// page wait through retries on a dead endpoint is what made translations
				// show up minutes late.
				return await translateViaEdge(texts, target)
			} catch (error) {
				// Any failure of route 1 — including a throttled one — hands the rest
				// of the session to Bing. Keeping the batch waiting on the route that
				// just failed is what made the first request of a page never arrive.
				edgeAuthBroken = true
				console.warn('Edge translator unavailable, falling back to Bing Translator:', error)
			}
		}

		return translateViaBing(texts, target)
	},
}
