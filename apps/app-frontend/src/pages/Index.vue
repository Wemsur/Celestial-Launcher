<script setup lang="ts">
import { BoxIcon, FolderSearchIcon, PlayIcon, PlusIcon } from '@modrinth/assets'
import {
	Button, defineMessages, injectNotificationManager,
	NavTabs, NewModal as Modal,
	StyledInput,
	useVIntl,
} from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { computed, inject, onActivated, onUnmounted, ref, shallowRef, watch, watchEffect } from 'vue'
import dayjs from 'dayjs'

import ContextMenu from '@/components/ui/context-menu/index.vue'
import LibrarySection from '@/components/ui/library/index.vue'
import WelcomeScreen from '@/components/ui/WelcomeScreen.vue'
import RecentWorldsList from '@/components/ui/world/RecentWorldsList.vue'
import { useAppEvent } from '@/composables/use-app-event'
import { useAppSettings } from '@/composables/use-app-settings.ts'
import type { InstanceFormat, LibraryInfo } from '@/helpers/library'
import { library_add, library_list, library_default_path, library_set_active } from '@/helpers/library'
import { list } from '@/helpers/instance'
import type { GameInstance } from '@/helpers/types'
import { useRootBreadcrumb } from '@/providers/breadcrumbs'
import { injectOnboardingChecklist } from '@/providers/onboarding-checklist'

defineOptions({
	name: 'LibraryPage',
})

const { handleError } = injectNotificationManager()
const { formatMessage } = useVIntl()
const { hasCreatedInstance, isReady } = injectOnboardingChecklist()
const showCreationModal = inject<() => void>('showCreationModal')
const pageOptions = ref<InstanceType<typeof ContextMenu>>()
const appSettings = useAppSettings()

const messages = defineMessages({
	home: {
		id: 'app.navigation.home',
		defaultMessage: 'Home',
	},
	newInstance: {
		id: 'app.library.context-menu.create-instance',
		defaultMessage: 'New instance',
	},
})

const homeBreadcrumb = useRootBreadcrumb({
	slot: 'root',
	id: 'home',
	label: formatMessage(messages.home),
	to: '/',
	visual: { type: 'icon', component: PlayIcon },
})
onActivated(homeBreadcrumb.reset)

// ── Instances ────────────────────────────────────────────────────────────────

const instances = ref<GameInstance[]>([])
let latestInstanceFetch = 0

const recentInstances = computed(() =>
	instances.value
		.slice()
		.sort((a, b) => dayjs(b.last_played ?? b.created).diff(dayjs(a.last_played ?? a.created))),
)

async function fetchInstances(filter?: string) {
	const fetchId = ++latestInstanceFetch
	try {
		const nextInstances = await list(filter).catch(() => [])
		if (fetchId === latestInstanceFetch) {
			instances.value = nextInstances
		}
	} catch (error: unknown) {
		if (fetchId === latestInstanceFetch) {
			handleError(error instanceof Error ? error : new Error(String(error)))
		}
	}
}

if (hasCreatedInstance.value) {
	await fetchInstances()
}

useAppEvent('instance', () => fetchInstances(activeTab.value === 'all' ? undefined : activeTab.value))
useAppEvent('instance_groups_changed', () => fetchInstances(activeTab.value === 'all' ? undefined : activeTab.value))

// ── Library tabs ─────────────────────────────────────────────────────────────

const libraries = shallowRef<LibraryInfo[]>([])
let defaultLibraryPath: string | null = null

const loadLibraries = async () => {
	try {
		const [config, defaultPath] = await Promise.all([
			library_list(),
			library_default_path().catch(() => null),
		])
		libraries.value = config.libraries
		defaultLibraryPath = defaultPath ?? null
		// Restore active library tab (skip if 'all' or path no longer exists)
		const saved = config.active_library_path
		if (saved && libraries.value.some((l) => l.path === saved)) {
			activeTab.value = saved
		}
	} catch (e) {
		handleError(e instanceof Error ? e : new Error(String(e)))
	}
}
loadLibraries()

const libDisplayLabel = (lib: LibraryInfo): string => {
	if (lib.name) return lib.name
	if (defaultLibraryPath) {
		const normDefault = defaultLibraryPath.replace(/\\/g, '/').toLowerCase().replace(/\/+$/, '')
		const normLib = lib.path.replace(/\\/g, '/').toLowerCase().replace(/\/+$/, '')
		if (normLib === normDefault || normLib === normDefault + '/profiles') {
			return '默认库'
		}
	}
	return lib.path.split('/').pop()?.split('\\').pop() ?? lib.path
}

const activeTab = ref('all')

const tabLinks = computed(() => {
	const tabs = [{ label: '全部实例', href: 'all' }]
	for (const lib of libraries.value) {
		tabs.push({ label: libDisplayLabel(lib), href: lib.path })
	}
	return tabs
})

const activeTabIndex = computed(() => {
	if (activeTab.value === 'all') return 0
	return tabLinks.value.findIndex((t) => t.href === activeTab.value)
})

const handleTabClick = (index: number, tab: { href: string }) => {
	activeTab.value = tab.href
	if (tab.href !== 'all') {
		library_set_active(tab.href).catch(() => {})
	} else {
		library_set_active('').catch(() => {})
	}
}

// Switching tab fetches instances for that library
watch(activeTab, async (tab) => {
	fetchInstances(tab === 'all' ? undefined : tab)
})

// ── Add library modal ───────────────────────────────────────────────────────

const addLibraryPath = ref('')
const addLibraryFormat = ref<InstanceFormat>('modrinth')
const addLibraryName = ref('')
const addLibraryModalRef = ref<InstanceType<typeof Modal> | null>(null)

async function pickFolder() {
	const result = await open({ directory: true, multiple: false })
	if (result && typeof result === 'string') {
		addLibraryPath.value = result
	}
}

async function addLibrary() {
	if (!addLibraryPath.value.trim()) return
	try {
		await library_add(addLibraryPath.value.trim(), addLibraryFormat.value,
			addLibraryName.value.trim() || undefined)
		closeAddLibraryModal()
		addLibraryPath.value = ''
		addLibraryName.value = ''
		await loadLibraries()
		fetchInstances(activeTab.value === 'all' ? undefined : activeTab.value)
	} catch (e) {
		handleError(e instanceof Error ? e : new Error(String(e)))
	}
}

function closeAddLibraryModal() {
	addLibraryModalRef.value?.hide()
}

// ── Context menu ─────────────────────────────────────────────────────────────

function openPageContextMenu(event: MouseEvent) {
	if (
		!(event.target instanceof HTMLElement) ||
		!event.target.hasAttribute('data-library-page-background')
	) {
		return
	}
	event.preventDefault()
	event.stopPropagation()
	pageOptions.value?.showMenu(event, {}, [{ name: 'new_instance' }])
}

function handlePageOption({ option }: { option: string }) {
	if (option === 'new_instance') {
		showCreationModal?.()
	}
}
</script>

<template>
	<WelcomeScreen v-if="isReady && !hasCreatedInstance" />
	<div
		v-else-if="isReady"
		data-library-page-background
		class="flex flex-col gap-3 p-6"
		@contextmenu="openPageContextMenu"
	>
		<!-- Multi-library tabs + Add library button -->
		<div class="flex items-center gap-4">
			<NavTabs
				mode="local"
				:links="tabLinks"
				:active-index="activeTabIndex"
				@tab-click="handleTabClick"
			/>
			<button
				v-tooltip.right="'添加库'"
				:aria-label="'添加库'"
				class="btn btn-brand rounded-full w-8 h-8 p-0 flex items-center justify-center transition-all hover:scale-105"
				@click="addLibraryModalRef?.show()"
			>
				<PlusIcon class="size-4" />
			</button>
		</div>

		<!-- Add Library Modal -->
		<Modal
			ref="addLibraryModalRef"
			header="添加库"
			:closable="false"
			noblur
		>
			<div class="flex flex-col gap-4 w-[500px]">
				<div class="flex flex-col gap-1">
					<h2 class="m-0 text-lg font-semibold text-contrast">
						库名称（可选）
					</h2>
					<StyledInput
						v-model="addLibraryName"
						placeholder="留空则使用文件夹名"
						type="text"
						wrapper-class="w-full"
					/>
				</div>
				<h2 class="m-0 text-lg font-semibold text-contrast">
					库位置
				</h2>
				<StyledInput
					v-model="addLibraryPath"
					placeholder="选择文件夹或输入路径"
					:icon="BoxIcon"
					type="text"
					wrapper-class="w-full"
				>
					<template #right>
						<Button type="quiet" @click="pickFolder" v-tooltip="'浏览文件夹'" :aria-label="'浏览文件夹'" class="ml-1.5">
							<FolderSearchIcon aria-hidden="true" />
						</Button>
					</template>
				</StyledInput>
				<div class="flex flex-col gap-1">
					<span class="text-sm font-medium text-primary">格式</span>
					<DropdownSelect
						v-model="addLibraryFormat"
						name="Library Format"
						:options="['modrinth', 'minecraft']"
						:display-name="(opt) => opt === 'modrinth' ? 'Modrinth (默认)' : 'Minecraft (.minecraft)'"
					>
						<span class="text-secondary">{{ addLibraryFormat === 'modrinth' ? 'Modrinth (默认)' : 'Minecraft (.minecraft)' }}</span>
					</DropdownSelect>
				</div>
				<div class="flex justify-end gap-2 pt-2">
					<Button type="outlined" @click="closeAddLibraryModal">取消</Button>
					<Button
						type="colored" color="brand"
						:disabled="!addLibraryPath.trim()"
						@click="addLibrary"
					>添加</Button>
				</div>
			</div>
		</Modal>

		<!-- World list (top of page, above library) -->
		<RecentWorldsList
			v-if="recentInstances?.length > 0 && appSettings.getFeatureFlag('worlds_in_home')"
			:recent-instances="recentInstances"
		/>

		<!-- Library Section -->
		<LibrarySection :instances="instances" :library-path="activeTab === 'all' ? undefined : activeTab" />

		<ContextMenu ref="pageOptions" @option-clicked="handlePageOption">
			<template #new_instance> <PlusIcon /> {{ formatMessage(messages.newInstance) }} </template>
		</ContextMenu>
	</div>
</template>
