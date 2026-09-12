<script setup lang="ts">
import type { Labrinth } from '@modrinth/api-client'
import { ChevronRightIcon, InfoIcon, Settings2Icon, UsersIcon, WrenchIcon } from '@modrinth/assets'
import {
	Avatar,
	commonMessages,
	defineMessage,
	TabbedModal,
	type TabbedModalTab,
	UnsavedChangesPopup,
	useVIntl,
} from '@modrinth/ui'
import type { PlatformTag } from '@modrinth/utils'
import { useQuery, useQueryClient } from '@tanstack/vue-query'
import { computed, nextTick, ref, watch } from 'vue'

import { get_project_v3 } from '@/helpers/cache'
import { get_linked_modpack_info, getInstanceIconUrl } from '@/helpers/instance'
import { get_loader_versions } from '@/helpers/metadata'
import { get_game_versions, get_loaders } from '@/helpers/tags'
import type { GameInstance } from '@/helpers/types'

import GeneralSettings from './general-settings.vue'
import InstallationSettings from './installation-settings.vue'
import { provideInstanceSettings } from './instance-settings-context.ts'
import SharingSettings from './sharing-settings.vue'
import SyncedOptionsSettings from './synced-options-settings.vue'

const { formatMessage } = useVIntl()
const queryClient = useQueryClient()

const props = defineProps<{
	instance: GameInstance
	offline?: boolean
}>()
const emit = defineEmits<{
	unlinked: []
}>()

const isMinecraftServer = ref(false)
const handleUnlinked = () => emit('unlinked')

const instanceRef = computed(() => props.instance)
const tabbedModal = ref<InstanceType<typeof TabbedModal> | null>(null)
let onAfterClose: (() => void) | undefined

const unsavedChangesPopup = ref<{ nudge: () => void } | null>(null)

const unsavedChangesController = ref<{
	hasChanges: () => boolean
	getOriginal: () => Record<string, unknown>
	getModified: () => Record<string, unknown>
	isSaving: () => boolean
	reset: () => void
	save: () => void | Promise<void>
} | null>(null)

const emptyUnsavedChangesState = {}
const originalUnsavedChangesState = computed(
	() => unsavedChangesController.value?.getOriginal() ?? emptyUnsavedChangesState,
)
const modifiedUnsavedChangesState = computed(
	() => unsavedChangesController.value?.getModified() ?? emptyUnsavedChangesState,
)
const savingUnsavedChanges = computed(
	() => unsavedChangesController.value?.isSaving() ?? false,
)
const hasUnsavedChanges = computed(
	() => unsavedChangesController.value?.hasChanges() ?? false,
)

function canLeaveCurrentTab(): boolean {
	if (!unsavedChangesController.value?.hasChanges()) return true
	unsavedChangesPopup.value?.nudge()
	return false
}

function hide(callback?: () => void): boolean {
	onAfterClose = callback
	const hidden = tabbedModal.value?.hide() ?? false
	if (!hidden) onAfterClose = undefined
	return hidden
}

function handleAfterHide() {
	const callback = onAfterClose
	onAfterClose = undefined
	callback?.()
}

function resetUnsavedChanges(): void {
	unsavedChangesController.value?.reset()
}

function saveUnsavedChanges(): void {
	void unsavedChangesController.value?.save()
}

provideInstanceSettings({
	instance: instanceRef,
	offline: props.offline,
	isMinecraftServer,
	onUnlinked: handleUnlinked,
	closeModal: hide,
	registerUnsavedChangesController: (controller) => {
		unsavedChangesController.value = controller
	},
})

watch(
	() => props.instance,
	(instance) => {
		isMinecraftServer.value = false
		if (instance.link?.project_id) {
			get_project_v3(instance.link.project_id, 'must_revalidate')
				.then((project: Labrinth.Projects.v3.Project | undefined) => {
					if (project?.minecraft_server != null) {
						isMinecraftServer.value = true
					}
				})
				.catch(() => {})
		}
	},
	{ immediate: true },
)

const tabs = computed<TabbedModalTab[]>(() => [
	{
		name: defineMessage({
			id: 'instance.settings.tabs.general',
			defaultMessage: 'General',
		}),
		icon: InfoIcon,
		content: GeneralSettings,
	},
	{
		name: defineMessage({
			id: 'instance.settings.tabs.installation',
			defaultMessage: 'Installation',
		}),
		icon: WrenchIcon,
		content: InstallationSettings,
	},
	{
		name: defineMessage({
			id: 'instance.settings.tabs.settings-overrides',
			defaultMessage: 'Sync overrides',
		}),
		icon: Settings2Icon,
		content: SyncedOptionsSettings,
	},
	{
		name: defineMessage({
			id: 'instance.settings.tabs.sharing',
			defaultMessage: 'Sharing',
		}),
		icon: UsersIcon,
		content: SharingSettings,
		shown: props.instance.shared_instance?.role === 'owner' && !props.instance.quarantined,
	},
])

function getSupportedModpackLoaders() {
	return get_loaders().then((value: PlatformTag[]) =>
		value
			.filter((item) => item.supported_project_types.includes('modpack') || item.name === 'vanilla')
			.sort((a, b) => (a.name === 'vanilla' ? -1 : b.name === 'vanilla' ? 1 : 0)),
	)
}

// Preload
useQuery({
	queryKey: ['instance-settings', 'loader-versions', 'fabric'],
	queryFn: () => get_loader_versions('fabric'),
})
useQuery({
	queryKey: ['instance-settings', 'loader-versions', 'forge'],
	queryFn: () => get_loader_versions('forge'),
})
useQuery({
	queryKey: ['instance-settings', 'loader-versions', 'quilt'],
	queryFn: () => get_loader_versions('quilt'),
})
useQuery({
	queryKey: ['instance-settings', 'loader-versions', 'neo'],
	queryFn: () => get_loader_versions('neo'),
})
useQuery({
	queryKey: ['instance-settings', 'game-versions'],
	queryFn: get_game_versions,
})
useQuery({
	queryKey: ['instance-settings', 'loaders', 'modpack'],
	queryFn: getSupportedModpackLoaders,
})
useQuery({
	queryKey: computed(() => ['linkedModpackInfo', props.instance.id]),
	queryFn: () => get_linked_modpack_info(props.instance.id, 'stale_while_revalidate'),
	enabled: computed(() => !!props.instance.link?.project_id && !props.offline),
})

function show(tabIndex?: number) {
	if (props.instance.link?.project_id) {
		queryClient.prefetchQuery({
			queryKey: ['linkedModpackInfo', props.instance.id],
			queryFn: () => get_linked_modpack_info(props.instance.id, 'stale_while_revalidate'),
		})
	}
	tabbedModal.value?.show()
	if (tabIndex !== undefined) {
		nextTick(() => tabbedModal.value?.setTab(tabIndex))
	}
}

defineExpose({ show, hide })
</script>
<template>
	<TabbedModal
		ref="tabbedModal"
		:tabs="tabs"
		:on-after-hide="handleAfterHide"
		:max-width="'min(928px, calc(95vw - 10rem))'"
		:width="'min(928px, calc(95vw - 10rem))'"
		:before-hide="canLeaveCurrentTab"
		:before-tab-change="canLeaveCurrentTab"
		:floating-action-bar-shown="hasUnsavedChanges"
	>
		<template #title>
			<span class="flex items-center gap-2 text-lg font-semibold text-primary">
				<Avatar
					:src="getInstanceIconUrl(instance.icon_path)"
					size="24px"
					:tint-by="props.instance.id"
					pad-transparent-corners
				/>
				{{ instance.name }} <ChevronRightIcon />
				<span class="font-extrabold text-contrast">{{
					formatMessage(commonMessages.settingsLabel)
				}}</span>
			</span>
		</template>
		<template #floating-action-bar>
			<UnsavedChangesPopup
				ref="unsavedChangesPopup"
				:original="originalUnsavedChangesState"
				:modified="modifiedUnsavedChangesState"
				:saving="savingUnsavedChanges"
				inline
				@reset="resetUnsavedChanges"
				@save="saveUnsavedChanges"
			/>
		</template>
	</TabbedModal>
</template>
