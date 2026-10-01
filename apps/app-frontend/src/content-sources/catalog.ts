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
import type { ContentType } from './types'

/** Content types the built-in Modrinth source serves. */
export const MODRINTH_CONTENT_TYPES: ContentType[] = [
	'mod',
	'modpack',
	'resourcepack',
	'shader',
	'datapack',
]

export interface SelectableSource {
	id: string
	name: string
	contentTypes: ContentType[]
	/** Whether the source's plugin is enabled and loaded. */
	available: boolean
}

/**
 * Every installed source that is usable: enabled, with a readable manifest.
 * A source that is installed but broken is left out rather than shown as an
 * option that cannot answer.
 */
export function installedSelectableSources(): SelectableSource[] {
	return contentSourceSummaries.value
		.filter((summary) => summary.enabled && summary.manifest)
		.map((summary) => ({
			id: summary.id,
			name: summary.manifest?.name ?? summary.dir_name,
			contentTypes: summary.manifest?.content_types ?? [],
			available: true,
		}))
}

/** Every source Discover can ask, built-in first. */
export function selectableSources(): SelectableSource[] {
	return [
		{
			id: MODRINTH_SOURCE_ID,
			name: 'Modrinth',
			contentTypes: MODRINTH_CONTENT_TYPES,
			available: true,
		},
		...installedSelectableSources(),
	]
}

export function sourceSupportsType(
	source: SelectableSource,
	type: ContentType | string,
): boolean {
	return source.contentTypes.includes(type as ContentType)
}
