<template>
	<NewModal
		ref="modal"
		:header="formatMessage(messages.header)"
		max-width="520px"
		:on-hide="handleHidden"
	>
		<p class="m-0 text-secondary">{{ formatMessage(messages.description) }}</p>

		<div v-if="libraries.length === 0" class="mt-3 text-secondary">
			{{ formatMessage(messages.empty) }}
		</div>

		<div v-else class="mt-3 flex flex-col gap-2">
			<button
				v-for="library in libraries"
				:key="library.path"
				type="button"
				class="flex w-full cursor-pointer items-center justify-between gap-3 rounded-xl border border-solid bg-surface-3 px-3 py-2 text-left transition-colors"
				:class="selectedPath === library.path ? 'border-brand' : 'border-surface-5'"
				@click="selectedPath = library.path"
			>
				<div class="flex min-w-0 flex-col gap-0.5">
					<span class="flex min-w-0 items-center gap-2">
						<span class="min-w-0 break-words text-sm font-medium text-contrast">
							{{ library.label }}
						</span>
						<span class="shrink-0 rounded-full bg-button-bg px-2 py-[0.125rem] text-xs font-medium text-secondary">
							{{
								formatMessage(
									library.format === 'minecraft' ? messages.badgeMinecraft : messages.badgeModrinth,
								)
							}}
						</span>
					</span>
					<span class="min-w-0 break-all text-xs text-secondary">{{ library.path }}</span>
				</div>
			</button>
		</div>

		<template #actions>
			<div class="flex justify-end gap-2">
				<Button type="outlined" @click="handleCancel">
					<XIcon />
					{{ formatMessage(commonMessages.cancelButton) }}
				</Button>
				<Button type="colored" color="brand" :disabled="!selectedPath" @click="handleConfirm">
					<RightArrowIcon />
					{{ formatMessage(messages.confirm) }}
				</Button>
			</div>
		</template>
	</NewModal>
</template>

<script setup lang="ts">
import { RightArrowIcon, XIcon } from '@modrinth/assets'
import { Button, commonMessages, defineMessages, NewModal, useVIntl } from '@modrinth/ui'
import { ref } from 'vue'

import type { InstanceFormat } from '@/helpers/library'
import { library_default_path, library_list, libraryDisplayLabel } from '@/helpers/library'

const { formatMessage } = useVIntl()

const messages = defineMessages({
	header: {
		id: 'app.modal.library-select.header',
		defaultMessage: 'Choose a library',
	},
	description: {
		id: 'app.modal.library-select.description',
		defaultMessage: 'Select which library this instance should be installed into.',
	},
	empty: {
		id: 'app.modal.library-select.empty',
		defaultMessage: 'No libraries are configured. The default library will be used.',
	},
	badgeModrinth: {
		id: 'app.modal.library-select.badge-modrinth',
		defaultMessage: 'Modrinth',
	},
	badgeMinecraft: {
		id: 'app.modal.library-select.badge-minecraft',
		defaultMessage: '.minecraft',
	},
	confirm: {
		id: 'app.modal.library-select.confirm',
		defaultMessage: 'Install here',
	},
})

type LibrarySelection = {
	path: string
	format: InstanceFormat
}

type LibraryEntry = LibrarySelection & { label: string }
const modal = ref<InstanceType<typeof NewModal>>()
const libraries = ref<LibraryEntry[]>([])
const selectedPath = ref<string | null>(null)

let resolveSelection: ((value: LibrarySelection | null) => void) | null = null

function settle(value: LibrarySelection | null) {
	const resolve = resolveSelection
	resolveSelection = null
	resolve?.(value)
}

/**
 * Shows the picker and resolves with the chosen library, or `null` if the user
 * cancelled. Any pending previous request is cancelled first so we never leave
 * a dangling promise behind.
 */
async function pick(): Promise<LibrarySelection | null> {
	settle(null)

	const config = await library_list()
	// Same labels the home nav's library tabs show, so the picker names a
	// library the way the page it installs onto does. A missing default path
	// only costs the "默认库" wording, not the list.
	const defaultPath = await library_default_path().catch(() => null)
	libraries.value = config.libraries.map((library) => ({
		label: libraryDisplayLabel(library, defaultPath),
		path: library.path,
		format: (String(library.type).toLowerCase() === 'minecraft'
			? 'minecraft'
			: 'modrinth') as InstanceFormat,
	}))

	const persisted =
		typeof window !== 'undefined' ? localStorage.getItem('celestial-library-active-tab') : null
	const preferred = [config.active_library_path, persisted].find(
		(candidate) => candidate && libraries.value.some((library) => library.path === candidate),
	)
	selectedPath.value = preferred ?? libraries.value[0]?.path ?? null

	return await new Promise<LibrarySelection | null>((resolve) => {
		resolveSelection = resolve
		modal.value?.show()
	})
}

function handleCancel() {
	modal.value?.hide()
}

/** Covers ESC / backdrop dismissal as well as {@link handleCancel}. */
function handleHidden() {
	settle(null)
}

function handleConfirm() {
	const selected = libraries.value.find((library) => library.path === selectedPath.value)
	settle(selected ? { path: selected.path, format: selected.format } : null)
	modal.value?.hide()
}

defineExpose({
	pick,
})
</script>
