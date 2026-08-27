<script setup lang="ts">
import { PlusIcon, SearchIcon, SpinnerIcon, SquarePlusIcon, UpdatedIcon } from '@modrinth/assets'
import { Button, defineMessages, StyledInput, useVIntl } from '@modrinth/ui'
import { computed, inject, type Ref } from 'vue'

import FilterMenu from '@/components/ui/library/library-toolbar/filter-menu.vue'
import NewGroupModal from '@/components/ui/library/library-toolbar/new-group-modal.vue'
import SortMenu from '@/components/ui/library/library-toolbar/sort-menu.vue'
import { useLibrary } from '@/components/ui/library/use-library'

const { search, selectedLibraryInstances, openNewGroupModal } = useLibrary()
const showCreationModal = inject<() => void>('showCreationModal')
// Provided by the home page, which owns the instance list.
const refreshInstances = inject<() => Promise<void>>('refreshLibraryInstances')
const isRefreshing = inject<Ref<boolean>>('isRefreshingLibraryInstances')
const { formatMessage } = useVIntl()
const messages = defineMessages({
	search: { id: 'app.library.search.placeholder', defaultMessage: 'Search' },
	newGroup: { id: 'app.library.group.new', defaultMessage: 'New group' },
	newInstance: { id: 'app.library.instance.new', defaultMessage: 'New instance' },
	refresh: { id: 'app.library.refresh', defaultMessage: 'Refresh instances' },
})
const selectedInstanceIds = computed(
	() =>
		new Set([...selectedLibraryInstances.value.values()].map((selection) => selection.instanceId)),
)

function openNewGroup() {
	openNewGroupModal(selectedInstanceIds.value)
}
</script>

<template>
	<div class="flex flex-col gap-2">
		<div class="flex flex-wrap gap-2">
			<Button
				v-tooltip="formatMessage(messages.refresh)"
				:aria-label="formatMessage(messages.refresh)"
				:disabled="isRefreshing?.value"
				@click="refreshInstances?.()"
			>
				<SpinnerIcon v-if="isRefreshing?.value" class="animate-spin" />
				<UpdatedIcon v-else />
			</Button>
			<StyledInput
				v-model="search"
				:icon="SearchIcon"
				type="text"
				:placeholder="formatMessage(messages.search)"
				clearable
				wrapper-class="min-w-[16rem] flex-1"
			/>
			<Button @click="openNewGroup">
				<SquarePlusIcon />
				{{ formatMessage(messages.newGroup) }}
			</Button>
			<Button type="colored" color="brand" @click="showCreationModal?.()">
				<PlusIcon />
				{{ formatMessage(messages.newInstance) }}
			</Button>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			<SortMenu />
			<div class="mx-2 h-6 w-px bg-surface-5" />
			<FilterMenu />
		</div>
	</div>
	<NewGroupModal />
</template>
