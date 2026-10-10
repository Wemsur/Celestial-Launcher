/**
 * Turns content sources' declared filter groups into the sidebar's shape.
 *
 * Every enabled, loaded source may offer groups per content type — pixelmap's
 * site categories, say. The launcher's closed vocabulary for a section title is
 * the Modrinth category headers: `categories`, `features`,
 * `performance impact`. A source group declaring one of those headers is merged
 * into the built-in section of the same title (options deduplicated by option
 * id), so a shader source's "性能要求" ticks sit beside Modrinth's own rather
 * than in a second group of the same name. Any other header stands alone.
 *
 * Groups are keyed `source_<sourceId>_<groupId>` in the FilterType id space, a
 * namespace no built-in filter uses, so the selections can be told apart when
 * the query is fanned out: built-in types go to Modrinth, `source_` types go to
 * their source as `ContentQuery.filters`.
 */

import { ref, watch } from 'vue'

import i18n from '@/i18n.config'

import { selectableSources } from './catalog'
import { contentSourceProviders, sourceProvider } from './registry'
import type { ContentType, SourceFilterGroup } from './types'

/** The built-in section titles a source group may merge into. */
export const MERGEABLE_HEADERS = ['categories', 'features', 'performance impact']

export interface SourceFiltersState {
	groups: Ref<Record<string, SourceFilterGroup[]>>
	loading: Ref<boolean>
}

/** Cache of the last answers, so a tab revisit does not re-fetch the world. */
const cache = new Map<string, SourceFilterGroup[]>()

/**
 * Loads the filter groups of every active source for one content type. Watched,
 * so a source that finishes loading late still shows its sections.
 */
export function useSourceFilters(contentType: () => string): SourceFiltersState {
	const groups = ref<Record<string, SourceFilterGroup[]>>({})
	const loading = ref(false)
	let loadVersion = 0

	async function reload() {
		const type = contentType() as ContentType
		if (!type) return
		const locale = i18n.global.locale.value
		const version = ++loadVersion
		loading.value = true

		const active = selectableSources().filter(
			(source) => source.available && source.id !== 'modrinth' && source.contentTypes.includes(type),
		)

		const next: Record<string, SourceFilterGroup[]> = {}
		await Promise.all(
			active.map(async (source) => {
				const key = `${source.id}:${type}:${locale}`
				const provider = sourceProvider(source.id)
				if (!provider?.filters) return
				try {
					const answer = cache.has(key) ? cache.get(key)! : await provider.filters(type)
					cache.set(key, answer)
					if (answer.length > 0) next[source.id] = answer
				} catch (error) {
					// A source whose filters fail still browses; it just has no sections.
					console.warn(`[content-sources] "${source.id}" filters() failed:`, error)
				}
			}),
		)

		if (version === loadVersion) {
			groups.value = next
			loading.value = false
		}
	}

	watch(
		() => [
			contentType(),
			i18n.global.locale.value,
			selectableSources().map((source) => `${source.id}:${source.available}`),
			// Providers appear late — after the plugin module loads — so the
			// registry itself is part of the trigger: a source that finishes
			// loading after the tab opened still contributes its sections.
			[...contentSourceProviders.value.keys()].join(','),
		],
		() => {
			void reload()
		},
		{ immediate: true, deep: true },
	)

	return { groups, loading }
}

export interface SidebarGroup {
	/** The FilterType id the group is exposed under. */
	filterTypeId: string
	sourceId: string
	groupId: string
	header: string
	options: { id: string; label: string }[]
}

/**
 * The source groups for the current content type, shaped for the sidebar and
 * merged where a header matches a built-in one: one entry per header, options
 * concatenated and deduplicated by option id.
 */
export function flattenSourceGroups(
	perSource: Record<string, SourceFilterGroup[]>,
): SidebarGroup[] {
	const sections: SidebarGroup[] = []

	for (const [sourceId, groups] of Object.entries(perSource)) {
		for (const group of groups) {
			const header = group.header.trim().toLowerCase()
			const options = group.options.map((option) => ({
				id: option.id,
				label: option.label ?? option.id,
			}))
			const section: SidebarGroup = {
				filterTypeId: `source_${sourceId}_${group.id}`,
				sourceId,
				groupId: group.id,
				header,
				options,
			}
			sections.push(section)
		}
	}

	return sections
}

/**
 * The selected values of the sidebar's `source_`-namespaced filters, keyed by
 * the source they belong to. This is what rides to a source's search/browse as
 * `ContentQuery.filters`.
 */
export function sourceFiltersFromSelection(
	sections: SidebarGroup[],
	selected: { type: string; option: string; negative?: boolean }[],
): Record<string, Record<string, string[]>> {
	const result: Record<string, Record<string, string[]>> = {}
	for (const value of selected) {
		if (!value.type.startsWith('source_') || value.negative) continue
		const section = sections.find((candidate) => candidate.filterTypeId === value.type)
		if (!section) continue
		const perSource = (result[section.sourceId] ??= {})
		const options = (perSource[section.groupId] ??= [])
		if (!options.includes(value.option)) options.push(value.option)
	}
	return result
}
