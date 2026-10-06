/**
 * The list of sources Discover can pull from: the built-in Modrinth source plus
 * every installed content source, with what each one serves.
 *
 * Modrinth is not a plugin — it is the launcher's own backend path — but it is
 * presented as one source among the others so the picker, the merged list and
 * the multipliers all treat it the same way. This module is the single place
 * that knows the difference.
 */

import { MODRINTH_SOURCE_ID } from './preferences'
import { contentSourceSummaries } from './registry'
import { contentSourceAssetUrl } from './ipc'
import type { ContentSourceManifest, ContentType } from './types'

/** Content types the built-in Modrinth source serves. */
export const MODRINTH_CONTENT_TYPES: ContentType[] = [
	'mod',
	'modpack',
	'resourcepack',
	'shader',
	'datapack',
]

/** Modrinth's own brand green, used wherever its colour is called for. */
export const MODRINTH_COLOR = '#1bd96a'

/** The colour a source without one is drawn in: present but not a statement. */
export const NEUTRAL_SOURCE_COLOR = '#8b8b8b'

export interface SelectableSource {
	id: string
	name: string
	contentTypes: ContentType[]
	/** Whether the source's plugin is enabled and loaded. */
	available: boolean
	/** The source's theme colour, or null when it declared none. */
	color: string | null
	/** A displayable URL for the source's own icon, when it declared one. */
	iconUrl: string | null
	/** The directory the source's files live in, for resolving its icon. */
	path: string | null
}

/**
 * Every installed source that is usable: enabled, with a readable manifest.
 * A source that is installed but broken is left out rather than shown as an
 * option that cannot answer.
 */
export function installedSelectableSources(): SelectableSource[] {
	return contentSourceSummaries.value
		.filter((summary) => summary.enabled && summary.manifest)
		.map((summary) => {
			const manifest = summary.manifest
			const icon = manifest?.icon
			return {
				id: summary.id,
				name: manifest?.name ?? summary.dir_name,
				contentTypes: manifest?.content_types ?? [],
				available: true,
				color: manifest?.color ?? null,
				iconUrl:
					icon && summary.path ? contentSourceAssetUrl(summary.path, icon) : null,
				path: summary.path ?? null,
			}
		})
}

/** Every source Discover can ask, built-in first. */
export function selectableSources(): SelectableSource[] {
	return [
		{
			id: MODRINTH_SOURCE_ID,
			name: 'Modrinth',
			contentTypes: MODRINTH_CONTENT_TYPES,
			available: true,
			color: MODRINTH_COLOR,
			iconUrl: null,
			path: null,
		},
		...installedSelectableSources(),
	]
}

/**
 * What a source declared in its `manifest.json`, as the backend read it, or
 * null when no such source is installed.
 *
 * A source's registered provider carries only its own functions — the theme
 * colour, the content types and the body format all live here, next to the
 * installed-source list, which is the only place the backend puts them.
 */
export function installedManifest(sourceId: string): ContentSourceManifest | null {
	const summary = contentSourceSummaries.value.find(
		(candidate) => candidate.id === sourceId,
	)
	return summary?.manifest ?? null
}

export function sourceSupportsType(
	source: SelectableSource,
	type: ContentType | string,
): boolean {
	return source.contentTypes.includes(type as ContentType)
}

export interface SourceAppearance {
	name: string
	/** `#rrggbb`, or the neutral grey when the source declared none. */
	color: string
	/** A displayable URL for the source's own icon, when it has one. */
	iconUrl: string | null
	/** True for the built-in source, which is drawn with Modrinth's own mark. */
	isModrinth: boolean
}

/**
 * How a source should be drawn, for anything that shows one: its icon, its
 * colour, or both. A source the launcher cannot place (a card from a source
 * that has since been uninstalled) still gets a name and a neutral tint rather
 * than an empty box.
 */
export function sourceAppearance(sourceId: string): SourceAppearance {
	if (sourceId === MODRINTH_SOURCE_ID) {
		return {
			name: 'Modrinth',
			color: MODRINTH_COLOR,
			iconUrl: null,
			isModrinth: true,
		}
	}
	const source = selectableSources().find((candidate) => candidate.id === sourceId)
	return {
		name: source?.name ?? sourceId,
		color: source?.color ?? NEUTRAL_SOURCE_COLOR,
		iconUrl: source?.iconUrl ?? null,
		isModrinth: false,
	}
}

/**
 * A `background-image` that washes an area with a source's colour: half opaque
 * at the top edge, fully gone by the bottom one.
 *
 * Returned as a `background-image` rather than as an overlay element so it can
 * never paint over the content it sits behind, and so a rounded or scrolling
 * container clips it without anything extra. `undefined` for an unusable
 * colour, so a caller can bind it straight to `:style`.
 */
export function sourceTintStyle(
	color: string | null | undefined,
): { backgroundImage: string } | undefined {
	const hex = (color ?? '').trim().replace('#', '')
	if (hex.length !== 6) return undefined
	const r = Number.parseInt(hex.slice(0, 2), 16)
	const g = Number.parseInt(hex.slice(2, 4), 16)
	const b = Number.parseInt(hex.slice(4, 6), 16)
	if ([r, g, b].some(Number.isNaN)) return undefined

	return {
		backgroundImage: `linear-gradient(to bottom, rgba(${r}, ${g}, ${b}, 0.5), rgba(${r}, ${g}, ${b}, 0))`,
	}
}
