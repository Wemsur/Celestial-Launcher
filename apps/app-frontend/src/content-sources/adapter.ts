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
import type { ContentCard, ContentVersion } from './types'

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
	extra?: { shared?: boolean; otherSources?: string[] },
): SourcedSearchResult {
	const projectId = syntheticProjectId(card.sourceId, card.id)
	// The source's own name is *not* put among the tags: it has its own chip on
	// the card, and two copies of it would read as two sources. What goes in the
	// tags is what the source says about the content — its categories.
	const contentTags = card.tags ?? []
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
		categories: contentTags,
		display_categories: contentTags,
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
 * A neutral card that still carries the Modrinth hit it was built from.
 *
 * Modrinth rows render from the full hit, not the lossy neutral card, so the hit
 * has to travel with the card through the merge and the result cache — a
 * module-level side table would be empty on a cache hit, when the fetch that
 * filled it never ran.
 */
export type ModrinthContentCard = ContentCard & {
	__modrinthHit?: Labrinth.Search.v3.ResultSearchProject & { installed?: boolean }
}

/**
 * The reverse, for feeding Modrinth's own results into the merge. Only the
 * fields the merge and the multipliers read are carried over; the raw hit rides
 * along for the final render.
 */
export function fromSearchResult(
	result: Labrinth.Search.v3.ResultSearchProject & { installed?: boolean },
	contentType: string,
): ModrinthContentCard {
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
		__modrinthHit: result,
	}
}

/** The raw Modrinth hit a card was built from, if it is a Modrinth card. */
export function modrinthHitOf(
	card: ContentCard,
): (Labrinth.Search.v3.ResultSearchProject & { installed?: boolean }) | undefined {
	return (card as ModrinthContentCard).__modrinthHit
}

/** Whether a rendered row came from a source other than the built-in one. */
export function resultSourceId(
	result: Labrinth.Search.v3.ResultSearchProject,
): string | undefined {
	return (result as SourcedSearchResult).__source?.sourceId
}

/**
 * A source version in the shape `ProjectPageVersions` renders.
 *
 * Same reasoning as {@link toSearchResult}: that table is the launcher's version
 * list — its columns, filters, pagination and row actions — and rebuilding it
 * for sources would give the same data two different-looking homes. A source
 * describes a version in neutral terms, so the conversion is field renaming
 * plus the defaults the table reads unconditionally. Nothing here is invented:
 * a source that does not know a figure leaves it empty, and the cell renders
 * empty rather than a made-up zero.
 */
/**
 * A source version as the versions table renders it, plus the source's own page
 * for it — a field Modrinth's rows build from their slug instead of carrying.
 */
export type VersionRow = Labrinth.Versions.v3.Version & {
	displayUrlEnding: string
	url?: string
}

export function toVersionRow(version: ContentVersion): VersionRow {
	return {
		id: version.id,
		project_id: '',
		author_id: '',
		name: version.name ?? version.versionNumber ?? version.id,
		version_number: version.versionNumber ?? version.id,
		version_type: (version.channel ?? 'release') as Labrinth.Versions.v3.Version['version_type'],
		status: 'listed' as Labrinth.Versions.v2.VersionStatus,
		featured: false,
		dependencies: [],
		game_versions: version.gameVersions ?? [],
		loaders: version.loaders ?? [],
		date_published: version.publishedAt ?? '',
		downloads: version.downloads ?? 0,
		url: version.url,
		// `showFiles` is off, so the table never reads these; an empty list keeps
		// the type honest rather than fabricating file entries.
		files: [],
		displayUrlEnding: '',
	}
}

