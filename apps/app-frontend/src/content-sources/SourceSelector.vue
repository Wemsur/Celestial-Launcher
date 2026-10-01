<script setup lang="ts">
import { defineMessages, MultiSelect, useVIntl } from '@modrinth/ui'
import { computed } from 'vue'

import { selectableSources, sourceSupportsType } from '@/content-sources/catalog'
import { isSourceActive, setSourceActive } from '@/content-sources/preferences'

const props = defineProps<{
	/** The content type being browsed; sources that cannot serve it are greyed. */
	contentType: string
}>()

const { formatMessage } = useVIntl()

const messages = defineMessages({
	placeholder: {
		id: 'app.browse.content-sources.placeholder',
		defaultMessage: 'Sources',
	},
	selectAll: {
		id: 'app.browse.content-sources.select-all',
		defaultMessage: 'All sources',
	},
	searchPlaceholder: {
		id: 'app.browse.content-sources.search',
		defaultMessage: 'Search sources',
	},
})

const sources = computed(() => selectableSources())

/**
 * A source that does not serve the current content type is shown disabled
 * rather than hidden, so the picker is stable across tabs.
 */
const options = computed(() =>
	sources.value.map((source) => ({
		value: source.id,
		label: source.name,
		disabled: !sourceSupportsType(source, props.contentType),
	})),
)

/**
 * Only sources that serve the current type take part, but toggling writes back
 * to every source — so turning a source off in one tab leaves it off in the
 * others rather than being silently re-enabled.
 */
const selected = computed<string[]>({
	get: () =>
		sources.value
			.filter(
				(source) =>
					sourceSupportsType(source, props.contentType) && isSourceActive(source.id),
			)
			.map((source) => source.id),
	set: (next: string[]) => {
		for (const source of sources.value) {
			if (!sourceSupportsType(source, props.contentType)) continue
			setSourceActive(source.id, next.includes(source.id))
		}
	},
})
</script>

<template>
	<MultiSelect
		v-model="selected"
		:options="options"
		fit-content
		:max-tag-rows="1"
		:searchable="options.length > 8"
		:placeholder="formatMessage(messages.placeholder)"
		:search-placeholder="formatMessage(messages.searchPlaceholder)"
		include-select-all-option
		:select-all-label="formatMessage(messages.selectAll)"
	/>
</template>
