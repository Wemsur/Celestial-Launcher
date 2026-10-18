/**
 * Fans a browse request out to every active source and merges the answers.
 *
 * Three rules shape the merge, all of them the user's:
 *
 * - Items with the same name *and* the same content type are one entry, however
 *   many sources offer them, and Modrinth's copy of it is the one drawn —
 *   its artwork and counts are the launcher's own.
 *   Everything else keeps the order its own source gave it, so a single active
 *   source behaves exactly like it did before.
 * - The downloads sort is applied to each source's counts multiplied by that
 *   source's multiplier, so a small site's counts are not drowned by Modrinth's.
 *   A source that cannot supply the number behind the requested sort ranks last
 *   rather than being dropped.
 * - One source failing does not fail the page: its error is reported and the
 *   rest of the results are shown.
 *
 * Paging stays per-source: page N of the merged list is page N from each active
 * source, merged. A global ordering across sources would need every source to be
 * exhaustively fetched, which is not what a paginated browse should do.
 */

import { MODRINTH_SOURCE_ID, sourceMultiplier } from './preferences'
import type { ContentCard, ContentSort, ContentType } from './types'

export interface FanOutRequest {
	query?: string
	contentType: ContentType
	sort: ContentSort
	/** 1-based. */
	page: number
	limit: number
	gameVersion?: string
	loader?: string
	/**
	 * Per-source sidebar selections: source id → group id → option ids. Each
	 * source only ever sees its own slice, stripped before the query reaches it.
	 */
	filters?: Record<string, Record<string, string[]>>
}

/** One source's contribution to the merged page. */
export interface SourceOffer {
	sourceId: string
	card: ContentCard
	/** This source's adjusted download count, when it has one. */
	adjustedDownloads?: number
}

/** A card as the merged list shows it: one entry, every source that offers it. */
export interface MergedContentItem {
	/** The card the row is drawn from — the offer with the strongest signal. */
	card: ContentCard
	/** Every source offering this item, in the order they answered. */
	offers: SourceOffer[]
	/** True when more than one source carries it. */
	shared: boolean
}

export interface SourceFailure {
	sourceId: string
	message: string
}

export interface MergedResult {
	items: MergedContentItem[]
	/**
	 * The largest single source's total, not the sum.
	 *
	 * Paging is per-source — page N is page N from each active source — so the
	 * deepest page any source can serve is `ceil(total / limit)` for the source
	 * with the most results, and that is what the page control must advertise.
	 * Summing the totals instead would claim pages that no source can fill: two
	 * sources of 100 items each would advertise 10 pages when page 6 onward is
	 * empty. An upper bound on the merged catalogue size, when one is wanted, is
	 * the sum of the per-source totals; that is deliberately not what this is.
	 */
	total: number
	failures: SourceFailure[]
}

/** A source as the aggregator sees it: a source id plus its query function. */
export interface ActiveSourceQuery {
	sourceId: string
	query(request: FanOutRequest): Promise<{ items: ContentCard[]; total: number }>
}

export interface SearchOptions {
	/**
	 * Called as each source answers, with everything merged so far.
	 *
	 * The final result is still returned and is the authoritative one: this is
	 * only a way to paint the rows that have already arrived instead of showing
	 * nothing until the slowest source replies. A caller that ignores it behaves
	 * exactly as before.
	 *
	 * `complete` says whether every source has now answered, so the caller can
	 * tell "some results, more coming" from "this is the final list".
	 */
	onPartial?: (partial: MergedResult & { complete: boolean }) => void
}

/**
 * How many sources' answers have arrived, for ordering partial results.
 *
 * A source that rejected still counts: its absence is part of the answer, and
 * waiting on it would be wrong.
 */
interface SourceAnswer {
	sourceId: string
	offers: SourceOffer[]
	/** `-1` when the source failed, so it is reported rather than counted. */
	total: number
	message?: string
}

function mergeAnswers(answers: SourceAnswer[], request: FanOutRequest, complete: boolean) {
	const failures: SourceFailure[] = []
	const perSource: SourceOffer[][] = []
	let total = 0

	for (const answer of answers) {
		if (answer.total < 0) {
			failures.push({ sourceId: answer.sourceId, message: answer.message ?? '' })
			continue
		}
		total = Math.max(total, answer.total)
		perSource.push(answer.offers)
	}

	const singleSource = perSource.length <= 1
	const items = singleSource
		? (perSource[0] ?? []).map((offer) => ({ card: offer.card, offers: [offer], shared: false }))
		: merge(
				perSource,
				request.sort === 'relevance' && !request.query?.trim() ? 'downloads' : request.sort,
			)

	return { items, total, failures, complete }
}

export async function searchAcrossSources(
	sources: ActiveSourceQuery[],
	request: FanOutRequest,
	options: SearchOptions = {},
): Promise<MergedResult> {
	if (sources.length === 0) {
		return { items: [], total: 0, failures: [] }
	}

	const answers: SourceAnswer[] = []
	let settledCount = 0

	// Answered one at a time, in the order the sources reply: a partial result is
	// only useful if it grows, so each arrival is merged and handed over as it
	// happens. `Promise.allSettled` would have waited for the slowest source and
	// thrown the arrivals away.
	await Promise.all(
		sources.map(async (source) => {
			try {
				const result = await source.query(request)
				const multiplier = sourceMultiplier(source.sourceId)
				answers.push({
					sourceId: source.sourceId,
					total: result.total ?? result.items.length,
					offers: result.items.map((card) => ({
						sourceId: source.sourceId,
						card,
						adjustedDownloads:
							typeof card.downloads === 'number' ? card.downloads * multiplier : undefined,
					})),
				})
			} catch (error) {
				answers.push({
					sourceId: source.sourceId,
					total: -1,
					offers: [],
					message: error instanceof Error ? error.message : String(error),
				})
			}

			settledCount += 1
			options.onPartial?.(mergeAnswers(answers, request, settledCount === sources.length))
		}),
	)

	const final = mergeAnswers(answers, request, true)
	return { items: final.items, total: final.total, failures: final.failures }
}

/** Group same-name same-type items together, then order the result. */
function merge(perSource: SourceOffer[][], sort: ContentSort): MergedContentItem[] {
	const groups = new Map<string, SourceOffer[]>()

	for (const offers of perSource) {
		for (const offer of offers) {
			const key = `${offer.card.contentType}:${normalizeName(offer.card.name)}`
			const existing = groups.get(key)
			if (existing) {
				existing.push(offer)
			} else {
				groups.set(key, [offer])
			}
		}
	}

	const items: MergedContentItem[] = []
	for (const offers of groups.values()) {
		/*
		 * The row is drawn from Modrinth's offer whenever there is one: it is the
		 * launcher's own catalogue, its icon and artwork are the ones a reader
		 * recognises, and its counts are the ones the rest of the app quotes. Only
		 * when a source is the sole carrier of an item does its card stand in —
		 * and then the strongest signal picks between several such sources, so a
		 * merged row still shows whichever of them knows the item best.
		 */
		const primary =
			offers.find((offer) => offer.sourceId === MODRINTH_SOURCE_ID) ??
			offers.reduce((best, offer) =>
				(offer.adjustedDownloads ?? -1) > (best.adjustedDownloads ?? -1) ? offer : best,
			)
		items.push({ card: primary.card, offers, shared: offers.length > 1 })
	}

	return sortItems(items, sort)
}

/** Preserve source relevance order for text searches; otherwise compare shared metrics. */
function sortItems(items: MergedContentItem[], sort: ContentSort): MergedContentItem[] {
	if (sort === 'relevance') return items

	if (sort === 'newest' || sort === 'updated') {
		return [...items].sort((a, b) => compareDates(itemDate(b, sort), itemDate(a, sort)))
	}

	const metric = (item: MergedContentItem): number | undefined => {
		if (sort === 'downloads') {
			// The strongest offer's adjusted count across the merged entry.
			return item.offers.reduce<number | undefined>((best, offer) => {
				const value = offer.adjustedDownloads
				if (typeof value !== 'number') return best
				return best === undefined || value > best ? value : best
			}, undefined)
		}
		// Follows have no scale adjustment, so the primary card's count is used.
		return typeof item.card.follows === 'number' ? item.card.follows : undefined
	}

	return [...items].sort((a, b) => {
		const aValue = metric(a)
		const bValue = metric(b)
		// A source that cannot supply the number sorts last, not out.
		if (aValue === undefined && bValue === undefined) return 0
		if (aValue === undefined) return 1
		if (bValue === undefined) return -1
		return bValue - aValue
	})
}

function itemDate(item: MergedContentItem, sort: ContentSort): number | undefined {
	const iso = sort === 'newest' ? item.card.publishedAt : item.card.updatedAt
	if (!iso) return undefined
	const time = Date.parse(iso)
	return Number.isNaN(time) ? undefined : time
}

/** Ascending by the given timestamps; undated entries last either way. */
function compareDates(a: number | undefined, b: number | undefined): number {
	if (a === undefined && b === undefined) return 0
	if (a === undefined) return 1
	if (b === undefined) return -1
	return a - b
}

/** Case- and whitespace-insensitive, so "JEI" and "jei " merge. */
function normalizeName(name: string): string {
	return name.trim().toLowerCase().replace(/\s+/g, ' ')
}
