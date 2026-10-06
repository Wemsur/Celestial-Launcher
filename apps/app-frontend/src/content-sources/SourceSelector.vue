<script setup lang="ts">
/**
 * The content-source picker: which sources Discover pulls from.
 *
 * The trigger shows the selected sources as marks rather than names — with a
 * handful of sources selected, a row of names would not fit the tab row — so
 * each mark carries its source's name as a tooltip. The dropdown itself is a
 * fixed width so the panel does not resize as the selection changes.
 */
import { ChevronLeftIcon, XIcon } from '@modrinth/assets'
import { MultiSelect, SourceBadge, useVIntl, defineMessages } from '@modrinth/ui'
import { computed, defineComponent, h } from 'vue'

import { selectableSources, sourceAppearance } from '@/content-sources/catalog'
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
	clearAll: {
		id: 'app.browse.content-sources.clear',
		defaultMessage: 'Clear all',
	},
})

/** Marks beyond this many collapse into a `+N`, keeping the row one line. */
const MAX_VISIBLE_MARKS = 5

const sources = computed(() => selectableSources())

/**
 * Every installed source is offered, whatever the current tab shows. A source is
 * never disabled for serving something else: the tab row follows the selection,
 * so hiding a source here would put the type it serves out of reach.
 */
const options = computed(() =>
	sources.value.map((source) => {
		const appearance = sourceAppearance(source.id)
		return {
			value: source.id,
			label: appearance.name,
			// MultiSelect renders an option's `icon` as a component and passes it no
			// props, so each option carries a tiny component bound to its own source.
			icon: defineComponent({
				name: 'SourceOptionBadge',
				render: () =>
					h(SourceBadge, {
						label: appearance.name,
						color: appearance.color,
						iconUrl: appearance.iconUrl,
						isModrinth: appearance.isModrinth,
						size: '20px',
					}),
			}),
		}
	}),
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

/** The selected sources as appearance objects, for the trigger's marks. */
const selectedAppearances = computed(() =>
	sources.value
		.filter((source) => isSourceActive(source.id))
		.map((source) => ({ id: source.id, ...sourceAppearance(source.id) })),
)

const visibleMarks = computed(() => selectedAppearances.value.slice(0, MAX_VISIBLE_MARKS))
const overflowCount = computed(() =>
	Math.max(0, selectedAppearances.value.length - MAX_VISIBLE_MARKS),
)
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
		dropdown-width="180px"
		include-select-all-option
		:select-all-label="formatMessage(messages.selectAll)"
	>
		<!--
			A custom trigger, so the selected sources appear as marks rather than
			names. The clear button and chevron the default trigger draws have to be
			drawn here too, since providing this slot replaces that markup.
		-->
		<template
			#input-content="{ modelValue, selectedOptions, clearAll, isOpen, openDirection }"
		>
			<div class="flex min-h-8 max-w-[200px] flex-1 flex-wrap items-center gap-1.5 overflow-hidden">
				<span
					v-for="mark in visibleMarks"
					:key="mark.id"
					class="inline-flex items-center"
				>
					<SourceBadge
						:label="mark.name"
						:color="mark.color"
						:icon-url="mark.iconUrl"
						:is-modrinth="mark.isModrinth"
						size="18px"
						tooltip
					/>
				</span>
				<span
					v-if="overflowCount > 0"
					v-tooltip="
						selectedAppearances
							.slice(MAX_VISIBLE_MARKS)
							.map((mark) => mark.name)
							.join('、')
					"
					class="inline-flex cursor-default select-none items-center rounded-full border border-solid border-surface-5 bg-surface-4 px-2 py-1 text-sm font-medium text-secondary"
				>
					+{{ overflowCount }}
				</span>
				<span v-if="selectedOptions.length === 0" class="text-base font-medium text-primary opacity-50">
					{{ formatMessage(messages.placeholder) }}
				</span>
			</div>
			<div class="ml-2 flex shrink-0 items-center gap-1.5">
				<button
					v-if="modelValue.length > 0"
					type="button"
					class="flex cursor-pointer items-center justify-center rounded border-none bg-transparent p-0.5 text-secondary transition-all hover:text-contrast"
					:aria-label="formatMessage(messages.clearAll)"
					@click.stop="clearAll"
				>
					<XIcon class="size-5" />
				</button>
				<div
					v-if="modelValue.length > 0"
					class="h-5 w-[1px] shrink-0 bg-surface-5"
				></div>
				<ChevronLeftIcon
					class="size-5 shrink-0 text-secondary transition-transform duration-150"
					:class="isOpen ? (openDirection === 'down' ? 'rotate-90' : '-rotate-90') : '-rotate-90'"
				/>
			</div>
		</template>
	</MultiSelect>
</template>
