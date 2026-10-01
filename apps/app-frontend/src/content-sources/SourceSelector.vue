<script setup lang="ts">
import { defineMessages, MultiSelect, useVIntl } from '@modrinth/ui'
import { computed } from 'vue'

import { selectableSources } from '@/content-sources/catalog'
import { isSourceActive, setSourceActive } from '@/content-sources/preferences'

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
 * Every installed source is offered, whatever the current tab shows. A source is
 * never disabled for serving something else: the tab row follows the selection
 * (only the selected sources' types appear), so hiding a source here would put
 * the type it serves out of reach — a world-only source could never be selected
 * from a mods tab, and its tab would never appear.
 */
const options = computed(() =>
	sources.value.map((source) => ({ value: source.id, label: source.name })),
)

const selected = computed<string[]>({
	get: () =>
		sources.value.filter((source) => isSourceActive(source.id)).map((source) => source.id),
	set: (next: string[]) => {
		for (const source of sources.value) {
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
