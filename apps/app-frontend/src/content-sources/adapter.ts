/**
 * View adapter from the neutral content model to the shape the browse list
 * renders.
 *
 * This is a *view* concern, not the data model: the neutral {@link ContentCard}
 * stays canonical and every source — Modrinth included — produces it. But the
 * launcher's existing result card (tags, badges, install button, density) is
 * written against the search-result type, and reusing it is what keeps a merged
 * list looking like the rest of the app instead of a second, half-finished list.
 * So the conversion happens here, at the last step before rendering.
 *
 * A foreign card carries its source on `__source`, which the card component
 * ignores and the page reads to decide what a click means.
 */

import type { Labrinth } from '@modrinth/api-client'

import { MODRINTH_SOURCE_ID } from './preferences'
import type { ContentCard } from './types'

export interface SourcedSearchResult extends Labrinth.Search.v3.ResultSearchProject {
	/** The source this row came from, and its own id for the project. */
	__source?: { sourceId: string; id: string; shared: boolean; otherSources: string[] }
}

/** The synthetic project id a source's own id maps to. */
export function syntheticProjectId(sourceId: string, id: string): string {
	return `${sourceId}:${id}`
}

export function toSearchResult(
	card: ContentCard,
	extra?: { sourceLabel?: string; shared?: boolean; otherSources?: string[] },
): SourcedSearchResult {
	const projectId = syntheticProjectId(card.sourceId, card.id)
	return {
		project_id: projectId,
		project_types: [card.contentType],
		all_project_types: [card.contentType],
		slug: null,
		author: card.author ?? '',
		author_id: null,
		organization: null,
		organization_id: null,
		name: card.name,
		summary: card.summary ?? '',
		categories: card.tags ?? [],
		// The source's own name rides along as a display tag, so a merged row
		// says where it came from without a bespoke card component.
		display_categories: extra?.sourceLabel ? [extra.sourceLabel] : [],
		downloads: card.downloads ?? 0,
		follows: card.follows ?? 0,
		icon_url: card.iconUrl ?? null,
		date_created: card.publishedAt ?? '',
		date_modified: card.updatedAt ?? '',
		license: '',
		gallery: [],
		featured_gallery: null,
		color: null,
		loaders: card.loaders ?? [],
		disclosure_types: [],
		__source: {
			sourceId: card.sourceId,
			id: card.id,
			shared: extra?.shared ?? false,
			otherSources: extra?.otherSources ?? [],
		},
	}
}

/**
 * The reverse, for feeding Modrinth's own results into the merge. Only the
 * fields the merge and the multipliers read are carried over.
 */
export function fromSearchResult(
	result: Labrinth.Search.v3.ResultSearchProject,
	contentType: string,
): ContentCard {
	return {
		sourceId: MODRINTH_SOURCE_ID,
		id: result.project_id,
		contentType: contentType as ContentCard['contentType'],
		name: result.name,
		summary: result.summary,
		iconUrl: result.icon_url ?? undefined,
		author: result.author,
		downloads: result.downloads,
		follows: result.follows,
		publishedAt: result.date_created || undefined,
		updatedAt: result.date_modified || undefined,
		loaders: result.loaders,
		tags: result.display_categories ?? [],
	}
}

/** Whether a rendered row came from a source other than the built-in one. */
export function resultSourceId(
	result: Labrinth.Search.v3.ResultSearchProject,
): string | undefined {
	return (result as SourcedSearchResult).__source?.sourceId
}

