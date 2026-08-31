// Content translation for the discover pages.
//
// A module-level singleton, like `use-split-view`: the top bar button, the card
// list and the project detail page all read the same state. The switch is not
// persisted — it turns itself off as soon as the user navigates out of discover
// — but the translated strings are, in `<appdata>/translation/cache.json`.
import { computed, ref, shallowRef } from 'vue'
import type { RouteLocationNormalized, RouteLocationNormalizedLoaded } from 'vue-router'
import { useRouter } from 'vue-router'

import { isProjectDetailRoute } from '@/composables/use-split-view'
import { translateHtml } from '@/helpers/translation/html'
import {
	DEFAULT_TRANSLATION_SERVICE,
	getTranslationProvider,
	isTranslationServiceId,
	TRANSLATION_PROVIDERS,
	type TranslationServiceId,
} from '@/helpers/translation/services'
import { isTransientTranslationError, sleep } from '@/helpers/translation/shared'
import {
	clearTranslationCache,
	loadTranslationCache,
	loadTranslationSettings,
	saveTranslationCache,
	saveTranslationSettings,
	type TranslationCacheEntry,
} from '@/helpers/translation/store'
import i18n from '@/i18n.config'

type RouteLike = RouteLocationNormalized | RouteLocationNormalizedLoaded

/** Requests in flight at once. Low on purpose: these are free endpoints. */
const CONCURRENCY = 2
/** Rewriting the cache file is debounced by this much. */
const SAVE_DEBOUNCE_MS = 3000
/** A cache hit older than this gets its timestamp refreshed, so content that
 *  keeps being viewed does not expire out from under the user. */
const TOUCH_AFTER_MS = 24 * 60 * 60 * 1000
/** Tries per text before a throttled service is treated as a real failure. */
const MAX_ATTEMPTS = 3
/** How long every worker backs off after the service asked us to slow down. */
const COOLDOWN_MS = 8000

const enabled = ref(false)
const serviceId = ref<TranslationServiceId>(DEFAULT_TRANSLATION_SERVICE)
/** key -> translated text. Reactive, so views update as batches land. */
const translations = ref<Record<string, string>>({})
/** key -> epoch ms. Mirrors what gets written to disk; not reactive. */
const timestamps = new Map<string, number>()
/** key -> source text, waiting for a request. */
const queued = new Map<string, string>()
const inFlight = new Set<string>()
/** key -> requests spent on it, counting only throttled ones. */
const attempts = new Map<string, number>()
/** Failed this session; not retried until the service changes. */
const failed = new Set<string>()
const translationError = shallowRef<{ service: string; message: string } | null>(null)

let cacheLoadPromise: Promise<void> | null = null
let settingsLoadPromise: Promise<void> | null = null
let cacheDirty = false
let saveTimer: ReturnType<typeof setTimeout> | null = null
let drainScheduled = false
let workers = 0
let errorReported = false
let trackingRoute = false
/** Epoch ms until which every worker holds off. */
let cooldownUntil = 0

/** cyrb53. 53 bits plus the source length makes collisions negligible, and
 *  keeps the cache file keys short. */
function hash(text: string): string {
	let h1 = 0xdeadbeef
	let h2 = 0x41c6ce57

	for (let i = 0; i < text.length; i++) {
		const ch = text.charCodeAt(i)
		h1 = Math.imul(h1 ^ ch, 2654435761)
		h2 = Math.imul(h2 ^ ch, 1597334677)
	}

	h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507) ^ Math.imul(h2 ^ (h2 >>> 13), 3266489909)
	h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507) ^ Math.imul(h1 ^ (h1 >>> 13), 3266489909)

	return (4294967296 * (2097151 & h2) + (h1 >>> 0)).toString(36)
}

/** The launcher's UI language is the target language; there is no separate setting. */
const targetLocale = computed(() => i18n.global.locale.value)

/** `null` when the selected service cannot handle the launcher's language. */
const targetLang = computed<string | null>(() => {
	try {
		return getTranslationProvider(serviceId.value).targetLang(targetLocale.value)
	} catch {
		return null
	}
})

function keyFor(text: string): string {
	return `${serviceId.value}:${targetLang.value}:${text.length}:${hash(text)}`
}

/** Text that is already in the target language is left alone. */
function alreadyTargetLanguage(text: string): boolean {
	const lang = targetLang.value
	if (!lang || !lang.toLowerCase().startsWith('zh')) return false

	const cjk = text.match(/[一-鿿]/g)?.length ?? 0
	return cjk / text.length > 0.3
}

/** Keeps a still-used entry from expiring, without rewriting the file constantly. */
function touch(key: string): void {
	const now = Date.now()
	if (now - (timestamps.get(key) ?? 0) < TOUCH_AFTER_MS) return

	timestamps.set(key, now)
	markDirty()
}

/**
 * The single function the views call: returns the translation once there is one,
 * and the original text meanwhile, queueing it for the next batch.
 *
 * Safe to call during render — nothing reactive is written here.
 */
function translate(text: string): string {
	if (!enabled.value || !text) return text

	const source = text.trim()
	if (!source || !targetLang.value || alreadyTargetLanguage(source)) return text

	const key = keyFor(source)
	const hit = translations.value[key]
	if (hit !== undefined) {
		touch(key)
		return hit
	}

	if (!failed.has(key) && !inFlight.has(key) && !queued.has(key)) {
		queued.set(key, source)
		scheduleDrain()
	}

	return text
}

/** Collects everything a single render pass asked for into one batch. */
function scheduleDrain(): void {
	if (drainScheduled) return

	drainScheduled = true
	setTimeout(() => {
		drainScheduled = false
		void drain()
	}, 0)
}

async function drain(): Promise<void> {
	// The disk cache may already hold most of what was just queued.
	await ensureCacheLoaded()

	while (workers < CONCURRENCY && queued.size > 0) {
		workers++
		void runWorker()
	}
}

async function runWorker(): Promise<void> {
	try {
		for (;;) {
			// A rate-limited service asked everyone to wait, not just this batch.
			const wait = cooldownUntil - Date.now()
			if (wait > 0) await sleep(wait)

			const batch = takeBatch()
			if (!batch) return
			await runBatch(batch)
		}
	} finally {
		workers--
	}
}

interface Batch {
	keys: string[]
	texts: string[]
	/** Captured here so switching service mid-request cannot mix the results. */
	service: TranslationServiceId
	target: string
}

/** Fills one request up to the provider's item and character limits. */
function takeBatch(): Batch | null {
	const target = targetLang.value
	if (!enabled.value || !target) {
		queued.clear()
		return null
	}

	const service = serviceId.value
	const provider = getTranslationProvider(service)
	const keys: string[] = []
	const texts: string[] = []
	let chars = 0

	for (const [key, text] of queued) {
		if (translations.value[key] !== undefined || inFlight.has(key)) {
			queued.delete(key)
			continue
		}

		const full = keys.length >= provider.maxItems || chars + text.length > provider.maxChars
		// A single text longer than the limit still has to go out; the provider
		// splits it itself.
		if (keys.length > 0 && full) break

		queued.delete(key)
		inFlight.add(key)
		keys.push(key)
		texts.push(text)
		chars += text.length
	}

	return keys.length > 0 ? { keys, texts, service, target } : null
}

async function runBatch(batch: Batch): Promise<void> {
	const provider = getTranslationProvider(batch.service)

	try {
		const results = await provider.translate(batch.texts, batch.target)
		// One assignment per batch, so views re-render once rather than per string.
		const next = { ...translations.value }
		const now = Date.now()

		batch.keys.forEach((key, index) => {
			const translated = results[index]
			// `null` is the provider saying it could not do this one while the rest
			// of the batch went through. Remember it, or the next render re-queues it.
			if (typeof translated !== 'string' || !translated) {
				failed.add(key)
				return
			}

			next[key] = translated
			timestamps.set(key, now)
			attempts.delete(key)
		})

		translations.value = next
		markDirty()
	} catch (error) {
		if (isTransientTranslationError(error) && requeue(batch)) {
			// Throttled, not broken: wait it out and keep the text in the queue.
			cooldownUntil = Date.now() + COOLDOWN_MS
			console.warn('Translation batch throttled, retrying later:', error)
		} else {
			// A failing free endpoint stays failing, and retrying it would burn the
			// quota that still works for other strings.
			for (const key of batch.keys) failed.add(key)
			reportError(provider.label, error)
		}
	} finally {
		for (const key of batch.keys) inFlight.delete(key)
	}
}

/**
 * Puts a throttled batch back in the queue. Returns false once every text in it
 * has used up its tries, so the caller can report it as a real failure.
 */
function requeue(batch: Batch): boolean {
	let requeued = 0

	batch.keys.forEach((key, index) => {
		const spent = (attempts.get(key) ?? 0) + 1
		attempts.set(key, spent)

		if (spent < MAX_ATTEMPTS) {
			queued.set(key, batch.texts[index])
			requeued++
		} else {
			failed.add(key)
		}
	})

	return requeued > 0
}

/** Surfaced once per session: a broken service would otherwise spam the user. */
function reportError(service: string, error: unknown): void {
	console.error('Translation request failed:', error)
	if (errorReported) return

	errorReported = true
	translationError.value = {
		service,
		message: error instanceof Error ? error.message : String(error),
	}
}

function markDirty(): void {
	cacheDirty = true
	if (saveTimer) return

	saveTimer = setTimeout(() => {
		saveTimer = null
		void flushCache()
	}, SAVE_DEBOUNCE_MS)
}

async function flushCache(): Promise<void> {
	if (!cacheDirty) return
	cacheDirty = false

	const now = Date.now()
	const cache = new Map<string, TranslationCacheEntry>()
	for (const [key, translated] of Object.entries(translations.value)) {
		cache.set(key, { t: translated, at: timestamps.get(key) ?? now })
	}

	await saveTranslationCache(cache)
}

function ensureCacheLoaded(): Promise<void> {
	cacheLoadPromise ??= loadTranslationCache().then((entries) => {
		if (entries.size === 0) return

		const merged = { ...translations.value }
		for (const [key, entry] of entries) {
			// Anything already translated this session is at least as fresh.
			if (merged[key] !== undefined) continue

			merged[key] = entry.t
			timestamps.set(key, entry.at)
		}

		translations.value = merged
	})

	return cacheLoadPromise
}

function ensureSettingsLoaded(): Promise<void> {
	settingsLoadPromise ??= loadTranslationSettings().then((settings) => {
		if (isTranslationServiceId(settings.service)) serviceId.value = settings.service
	})

	return settingsLoadPromise
}

/**
 * Switching service is instant. Cached entries of the old service stay valid
 * because the service id is part of the cache key.
 */
async function setService(id: TranslationServiceId): Promise<void> {
	if (serviceId.value === id) return

	serviceId.value = id
	queued.clear()
	failed.clear()
	attempts.clear()
	cooldownUntil = 0
	errorReported = false
	translationError.value = null

	await saveTranslationSettings({ service: id })

	if (enabled.value) scheduleDrain()
}

async function enable(): Promise<void> {
	enabled.value = true
	translationError.value = null
	// A retry the user asked for: give the strings that were throttled or refused
	// earlier another chance.
	failed.clear()
	attempts.clear()
	errorReported = false

	await ensureSettingsLoaded()
	await ensureUsableService()
	await ensureCacheLoaded()
}

/**
 * Not every service covers every launcher language — TranSmart, the default,
 * offers seventeen. A service that cannot handle the current one would translate
 * nothing at all and say nothing about it, so switch to one that can.
 */
async function ensureUsableService(): Promise<void> {
	if (targetLang.value) return

	const usable = TRANSLATION_PROVIDERS.find((provider) => {
		try {
			return Boolean(provider.targetLang(targetLocale.value))
		} catch {
			return false
		}
	})

	if (usable && isTranslationServiceId(usable.id)) await setService(usable.id)
}

function disable(): void {
	if (!enabled.value) return

	enabled.value = false
	queued.clear()
	translationError.value = null
	void flushCache()
}

function toggle(): void {
	if (enabled.value) {
		disable()
	} else {
		void enable()
	}
}

/** Drops both the file and everything held in memory; visible text re-requests. */
async function clearCache(): Promise<void> {
	translations.value = {}
	timestamps.clear()
	failed.clear()
	attempts.clear()
	queued.clear()
	cacheDirty = false
	cacheLoadPromise = null

	await clearTranslationCache()
}

/** Where the button is offered: the discover list and the project pages it opens. */
export function isTranslationScope(route: RouteLike): boolean {
	return route.path.startsWith('/browse') || isProjectDetailRoute(route)
}

export function useContentTranslation() {
	const router = useRouter()

	if (!trackingRoute) {
		trackingRoute = true
		// Leaving discover turns it off; the cached strings survive, the switch does not.
		router.afterEach((to) => {
			if (!isTranslationScope(to)) disable()
		})
	}

	// The chosen service is needed before the first click, for the settings page.
	void ensureSettingsLoaded()

	return {
		enabled,
		serviceId,
		error: translationError,
		/** Plain text: card summaries, the project header summary. */
		translate,
		/** Rendered markdown: only text nodes are replaced. */
		translateHtml: (html: string) => (enabled.value ? translateHtml(html, translate) : html),
		toggle,
		setService,
		clearCache,
	}
}
