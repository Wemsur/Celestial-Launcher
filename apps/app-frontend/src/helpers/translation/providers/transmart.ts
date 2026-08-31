// Tencent's TranSmart (交互翻译).
//
// The endpoint behind transmart.qq.com's own web UI. It batches, it is fast
// from mainland China, and unlike the other options here it needs no token
// handshake — only a browser UA and the matching Referer/Origin.
//
// It answers failures with HTTP 200 and a `header.ret_code`, so the error path
// repeats what the server said instead of a generic "unexpected payload" —
// without that there is no way to tell a bad request shape from a block.
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'

import {
	BROWSER_USER_AGENT,
	describeFailure,
	primarySubtag,
	TransientTranslationError,
	type TranslationProvider,
	withRetry,
} from '../shared'

const TRANSLATE_URL = 'https://transmart.qq.com/api/imt'
const ORIGIN = 'https://transmart.qq.com'
const REFERER = 'https://transmart.qq.com/zh-CN/index'

/** Languages the web UI offers; anything else is rejected server-side. */
const SUPPORTED = new Set([
	'zh',
	'en',
	'ja',
	'ko',
	'ru',
	'fr',
	'de',
	'es',
	'it',
	'tr',
	'pt',
	'vi',
	'id',
	'th',
	'ms',
	'ar',
	'hi',
])

/**
 * Mimics the key the web UI generates once per page load. Kept for the whole
 * launcher session: a new key on every request looks like a bot.
 */
let clientKey: string | null = null

function getClientKey(): string {
	if (!clientKey) {
		clientKey = `browser-Chrome-131.0.0-Windows 10-${crypto.randomUUID()}-${Date.now()}`
	}
	return clientKey
}

interface TranSmartResponse {
	header?: { ret_code?: string; message?: string; msg?: string }
	auto_translation?: unknown
	translation?: unknown
}

/** Both keys have been seen in the wild depending on the requested `fn`. */
function pickTranslations(payload: TranSmartResponse): unknown[] | null {
	if (Array.isArray(payload?.auto_translation)) return payload.auto_translation
	if (Array.isArray(payload?.translation)) return payload.translation
	return null
}

async function post(texts: string[], target: string): Promise<string[]> {
	const response = await tauriFetch(TRANSLATE_URL, {
		method: 'POST',
		headers: {
			'Content-Type': 'application/json',
			Accept: 'application/json',
			'User-Agent': BROWSER_USER_AGENT,
			Origin: ORIGIN,
			Referer: REFERER,
		},
		body: JSON.stringify({
			header: {
				fn: 'auto_translation',
				client_key: getClientKey(),
				session: '',
				user: '',
			},
			type: 'plain',
			model_category: 'normal',
			text_domain: '',
			source: { lang: 'auto', text_list: texts },
			target: { lang: target },
		}),
	})

	if (!response.ok) throw await describeFailure(response, 'TranSmart failed')

	const raw = await response.text()
	let payload: TranSmartResponse
	try {
		payload = JSON.parse(raw) as TranSmartResponse
	} catch {
		throw new Error(`TranSmart returned a non-JSON reply: ${raw.trim().slice(0, 180)}`)
	}

	const translated = pickTranslations(payload)
	if (!translated || translated.length !== texts.length) {
		// A refusal arrives as HTTP 200 with the reason in the header block.
		const reason =
			payload?.header?.message ?? payload?.header?.msg ?? payload?.header?.ret_code ?? ''
		const detail = reason || raw.trim().slice(0, 180)
		const message = `TranSmart refused the request: ${detail}`

		// Quota and frequency complaints are worth another go later.
		throw /limit|frequen|freq|busy|retry|quota|次数|频繁|繁忙/i.test(detail)
			? new TransientTranslationError(message)
			: new Error(message)
	}

	return translated.map((item, index) => (typeof item === 'string' ? item : texts[index]))
}

export const transmartProvider: TranslationProvider = {
	id: 'transmart',
	label: 'Tencent TranSmart',
	maxItems: 30,
	maxChars: 3000,
	targetLang(locale) {
		const code = primarySubtag(locale)
		if (!SUPPORTED.has(code)) throw new Error(`TranSmart does not support ${locale}`)
		return code
	},
	translate(texts, target) {
		return withRetry(() => post(texts, target))
	},
}
