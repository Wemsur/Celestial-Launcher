<script setup lang="ts">
import { ArrowUpDownIcon, LayoutGridIcon, SortAscIcon, SortDescIcon } from '@modrinth/assets'
import {
	Combobox,
	type ComboboxOption,
	defineMessages,
	type MessageDescriptor,
	useVIntl,
} from '@modrinth/ui'
import { computed } from 'vue'

import {
	type LibraryGroupBy,
	libraryGroupOptions,
	type LibrarySort,
	librarySortOptions,
	useLibrary,
} from '@/components/ui/library/use-library'

const { displayState, isSortAscending, toggleSortDirection } = useLibrary()
const { formatMessage } = useVIntl()

const messages = defineMessages({
	name: { id: 'app.library.sort.name', defaultMessage: 'Name' },
	loaderSort: { id: 'app.library.sort.loader', defaultMessage: 'Loader' },
	gameVersionSort: { id: 'app.library.sort.game-version', defaultMessage: 'Game version' },
	lastPlayed: { id: 'app.library.sort.last-played', defaultMessage: 'Last played' },
	hoursPlayed: { id: 'app.library.sort.hours-played', defaultMessage: 'Hours played' },
	dateCreated: { id: 'app.library.sort.date-created', defaultMessage: 'Date created' },
	dateModified: { id: 'app.library.sort.date-modified', defaultMessage: 'Date modified' },
	customGroup: { id: 'app.library.group-by.custom-group', defaultMessage: 'Custom group' },
	instanceType: { id: 'app.library.group-by.instance-type', defaultMessage: 'Instance type' },
	loader: { id: 'app.library.group-by.loader', defaultMessage: 'Loader' },
	gameVersion: { id: 'app.library.group-by.game-version', defaultMessage: 'Game version' },
	noGrouping: { id: 'app.library.group-by.none', defaultMessage: 'No grouping' },
	sortBy: { id: 'app.library.sort.label', defaultMessage: 'Sort by' },
	groupBy: { id: 'app.library.group-by.label', defaultMessage: 'Group by' },
	ascAlphabetical: { id: 'app.library.sort.direction.asc-alphabetical', defaultMessage: 'A–Z' },
	descAlphabetical: { id: 'app.library.sort.direction.desc-alphabetical', defaultMessage: 'Z–A' },
	ascVersion: {
		id: 'app.library.sort.direction.asc-version',
		defaultMessage: 'Oldest version first',
	},
	descVersion: {
		id: 'app.library.sort.direction.desc-version',
		defaultMessage: 'Newest version first',
	},
	ascRecency: {
		id: 'app.library.sort.direction.asc-recency',
		defaultMessage: 'Least recent first',
	},
	descRecency: {
		id: 'app.library.sort.direction.desc-recency',
		defaultMessage: 'Most recent first',
	},
	ascAmount: { id: 'app.library.sort.direction.asc-amount', defaultMessage: 'Fewest first' },
	descAmount: { id: 'app.library.sort.direction.desc-amount', defaultMessage: 'Most first' },
	ascDate: { id: 'app.library.sort.direction.asc-date', defaultMessage: 'Oldest first' },
	descDate: { id: 'app.library.sort.direction.desc-date', defaultMessage: 'Newest first' },
})

// What the up/down arrow means for each sort mode. The arrow always points in
// the direction of the underlying value: up = ascending, down = descending.
const sortDirectionLabels: Record<LibrarySort, { asc: MessageDescriptor; desc: MessageDescriptor }> =
	{
		Name: { asc: messages.ascAlphabetical, desc: messages.descAlphabetical },
		Loader: { asc: messages.ascAlphabetical, desc: messages.descAlphabetical },
		'Game version': { asc: messages.ascVersion, desc: messages.descVersion },
		'Last played': { asc: messages.ascRecency, desc: messages.descRecency },
		'Hours played': { asc: messages.ascAmount, desc: messages.descAmount },
		'Date created': { asc: messages.ascDate, desc: messages.descDate },
		'Date modified': { asc: messages.ascDate, desc: messages.descDate },
	}

const sortDirectionLabel = computed(() => {
	const labels = sortDirectionLabels[displayState.value.sortBy]
	return formatMessage(isSortAscending.value ? labels.asc : labels.desc)
})

const sortLabels = {
	Name: messages.name,
	'Last played': messages.lastPlayed,
	'Hours played': messages.hoursPlayed,
	'Date created': messages.dateCreated,
	'Date modified': messages.dateModified,
	Loader: messages.loaderSort,
	'Game version': messages.gameVersionSort,
}

const groupLabels = {
	Group: messages.customGroup,
	'Instance type': messages.instanceType,
	Loader: messages.loader,
	'Game version': messages.gameVersion,
	None: messages.noGrouping,
}

const sortOptions: ComboboxOption<LibrarySort>[] = librarySortOptions.map((option) => ({
	value: option,
	label: formatMessage(sortLabels[option]),
}))
const groupOptions: ComboboxOption<LibraryGroupBy>[] = libraryGroupOptions.map((option) => ({
	value: option.value,
	label: formatMessage(groupLabels[option.value]),
}))
</script>

<template>
	<Combobox
		v-model="displayState.sortBy"
		class="w-max"
		:options="sortOptions"
		:show-icon-in-selected="false"
		:max-height="320"
		dropdown-min-width="160px"
	>
		<template #prefix>
			<ArrowUpDownIcon class="size-5 text-primary" :aria-label="formatMessage(messages.sortBy)" />
		</template>
		<template #selected="{ label }">
			<span>{{ label }}</span>
		</template>
	</Combobox>
	<button
		v-tooltip="sortDirectionLabel"
		type="button"
		class="flex h-[40px] w-[40px] shrink-0 cursor-pointer items-center justify-center rounded-xl border-none bg-button-bg p-0 text-button-text transition-all hover:bg-button-bg hover:text-contrast active:scale-[0.97]"
		:aria-label="sortDirectionLabel"
		:aria-pressed="isSortAscending"
		@click="toggleSortDirection()"
	>
		<SortAscIcon v-if="isSortAscending" class="size-5" />
		<SortDescIcon v-else class="size-5" />
	</button>
	<Combobox
		v-model="displayState.group"
		class="w-max"
		:options="groupOptions"
		dropdown-min-width="160px"
		:show-icon-in-selected="false"
	>
		<template #prefix>
			<LayoutGridIcon class="size-5 text-primary" :aria-label="formatMessage(messages.groupBy)" />
		</template>
		<template #selected="{ label }">
			<span>{{ label }}</span>
		</template>
	</Combobox>
</template>
