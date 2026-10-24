/**
 * The shapes the recommendation block below the browse list works with.
 *
 * A card is flattened to what the block draws rather than carrying the neutral
 * `ContentCard`, so the block does not have to know how a source's card becomes
 * a route — the page that already does that hands it the finished link.
 */

import type { RouteLocationRaw } from 'vue-router'

export interface RecommendationCard {
	/** Unique within its source. */
	id: string
	name: string
	iconUrl?: string
	summary?: string
	tags?: string[]
	link: RouteLocationRaw
}

export interface RecommendationSource {
	sourceId: string
	/** The first batch, fetched along with the main search. */
	items: RecommendationCard[]
	/** The source's own total for the query. */
	total: number
}
