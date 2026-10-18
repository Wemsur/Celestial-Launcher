<script setup lang="ts">
import { CogIcon, PlayIcon, PlusIcon } from '@modrinth/assets'
import {
	AddLibraryModal,
	Button, ContextMenu, defineMessages, injectNotificationManager,
	Input,
	NavTabs, NewModal as Modal,
	useVIntl,
} from '@modrinth/ui'
import { computed, inject, onActivated, provide, type Ref, ref, shallowRef, watch } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'

import LibrarySection from '@/components/ui/library/index.vue'
import { libraryScrollTop } from '@/components/ui/library/view-state'
import NavButton from "@/components/ui/NavButton.vue";
import RecentWorldsList from '@/components/ui/world/RecentWorldsList.vue'
import { useAppEvent } from '@/composables/use-app-event'
import { useAppSettings } from '@/composables/use-app-settings.ts'
import { list } from '@/helpers/instance'
import type { LibraryInfo } from '@/helpers/library'
import { library_default_path, library_list, library_remove, library_reorder, library_set_active, library_update_name } from '@/helpers/library'
import type { GameInstance } from '@/helpers/types'
import { useRootBreadcrumb } from '@/providers/breadcrumbs'
import { injectOnboardingChecklist } from '@/providers/onboarding-checklist'

defineOptions({
	name: 'LibraryPage',
})

const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()
const { hasCreatedInstance, isReady } = injectOnboardingChecklist()
const showCreationModal = inject<() => void>('showCreationModal')
const pageOptions = ref<InstanceType<typeof ContextMenu>>()
const appSettings = useAppSettings()
onBeforeRouteLeave(() => {
	libraryScrollTop.value = document.querySelector('.app-viewport')?.scrollTop ?? 0
})

// Unfiltered and sorted most-recently-played first, provided by App.vue so the
// home page and the sidebar copy of "jump back in" share one `list()` call.
// "Jump in" is about the whole launcher, so it deliberately ignores the library
// tab in view.
const recentInstances = inject<Ref<GameInstance[]>>('recentInstances', ref([]))
const showJumpBackInHome = computed(() => appSettings.getFeatureFlag('worlds_in_home'))

const messages = defineMessages({
	home: {
		id: 'app.navigation.home',
		defaultMessage: 'Home',
	},
	newInstance: {
		id: 'app.library.context-menu.create-instance',
		defaultMessage: 'New instance',
	},
	library: { id: 'app.library.title', defaultMessage: 'Library' },
	libraryActionsLabel: {
		id: 'app.library.actions.label',
		defaultMessage: 'Library actions',
	},
})

// The home page is the breadcrumb root. `onActivated` fires when this component
// is re-entered from the KeepAlive cache (see App.vue's `<KeepAlive>`), which is
// what drops whatever the previous page pushed onto the stack — without it the
// title bar keeps showing the page you just came from.
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

useAppEvent('instance', () => {
	fetchInstances(activeTab.value === 'all' ? undefined : activeTab.value)
})
useAppEvent('instance_groups_changed', () => fetchInstances(activeTab.value === 'all' ? undefined : activeTab.value))

// Manual rescan, wired to the refresh button in the library toolbar. `list()`
// re-walks the libraries on disk on every call, so re-fetching is a full
// re-detection; the library list is reloaded too in case a library folder
// appeared or vanished.
const isRefreshingInstances = ref(false)

async function refreshInstances() {
	if (isRefreshingInstances.value) return
	isRefreshingInstances.value = true
	try {
		await loadLibraries()
		// A library tab may have disappeared while we were away.
		if (activeTab.value !== 'all' && !libraries.value.some((l) => l.path === activeTab.value)) {
			activeTab.value = 'all'
		}
		await fetchInstances(activeTab.value === 'all' ? undefined : activeTab.value)
	} finally {
		isRefreshingInstances.value = false
	}
}

provide('refreshLibraryInstances', refreshInstances)
provide('isRefreshingLibraryInstances', isRefreshingInstances)

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

// Drag-and-drop reordering of the library tabs. Tab 0 is the pinned "全部实例"
// entry, so both indices are shifted by one to index into `libraries`.
const handleTabReorder = async (fromIndex: number, toIndex: number) => {
	const from = fromIndex - 1
	const to = toIndex - 1
	const reordered = [...libraries.value]
	if (from < 0 || to < 0 || from >= reordered.length || to >= reordered.length) return

	const [moved] = reordered.splice(from, 1)
	reordered.splice(to, 0, moved)
	// Optimistic: the nav reflects the new order immediately; on failure we
	// reload from disk to undo it.
	libraries.value = reordered

	try {
		await library_reorder(reordered.map((lib) => lib.path))
	} catch (e) {
		await loadLibraries()
		handleError(e instanceof Error ? e : new Error(String(e)))
	}
}

// Switching tab fetches instances for that library
watch(activeTab, async (tab) => {
	fetchInstances(tab === 'all' ? undefined : tab)
})

// Mirror the active library into localStorage so the creation modal (and the
// library picker) can pre-select it. "all" means "no specific library", i.e.
// fall back to the default one.
watch(
	activeTab,
	(tab) => {
		if (typeof window === 'undefined') return
		if (tab && tab !== 'all') {
			localStorage.setItem('celestial-library-active-tab', tab)
		} else {
			localStorage.removeItem('celestial-library-active-tab')
		}
	},
	{ immediate: true },
)

// ── Add library modal ───────────────────────────────────────────────────────

// The dialog itself is shared with the library page and the creation flow, so
// the home toolbar only has to open it and reload the list it produces.
const addLibraryModalRef = ref<InstanceType<typeof AddLibraryModal> | null>(null)

async function onLibraryAdded() {
	await loadLibraries()
	await fetchInstances(activeTab.value === 'all' ? undefined : activeTab.value)
}

// ── Library settings modal ─────────────────────────────────────────────────

const librarySettingsModalRef = ref<InstanceType<typeof Modal> | null>(null)
const librarySettingsName = ref('')
const librarySettingsPath = ref('')
const librarySettingsRenameError = ref('')

function openLibrarySettings(path: string) {
	const lib = libraries.value.find((l) => l.path === path)
	if (!lib) return
	librarySettingsPath.value = path
	librarySettingsName.value = lib.name
	librarySettingsRenameError.value = ''
	librarySettingsModalRef.value?.show()
}

function closeLibrarySettingsModal() {
	librarySettingsModalRef.value?.hide()
}

async function saveLibraryName() {
	if (!librarySettingsPath.value.trim() || !librarySettingsName.value.trim()) return
	try {
		await library_update_name(librarySettingsPath.value, librarySettingsName.value)
		closeLibrarySettingsModal()
		await loadLibraries()
	} catch (e) {
		librarySettingsRenameError.value = e instanceof Error ? e.message : String(e)
	}
}

async function removeLibrary() {
	const path = librarySettingsPath.value
	if (!path) return
	// Never let the last library go: every install resolves a target library, so
	// an empty libraries.json would leave nowhere to create instances.
	if (libraries.value.length <= 1) return
	// Switch to "all" before removing so activeTab doesn't point to a deleted library
	if (activeTab.value === path) {
		activeTab.value = 'all'
		library_set_active('').catch(() => {})
	}
	try {
		await library_remove(path)
		closeLibrarySettingsModal()
		await loadLibraries()
		fetchInstances('all')
	} catch (e) {
		handleError(e instanceof Error ? e : new Error(String(e)))
	}
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
	pageOptions.value?.open(event, [
		{
			id: 'new_instance',
			label: formatMessage(messages.newInstance),
			icon: PlusIcon,
			action: () => showCreationModal?.(),
		},
	])
}
</script>

<template>
	<div
		v-if="isReady"
		data-library-page-background
		class="flex flex-col gap-3 p-6"
		@contextmenu="openPageContextMenu"
	>
		<!-- Add Library Modal -->
		<AddLibraryModal ref="addLibraryModalRef" @added="onLibraryAdded" />

		<!-- Library settings modal -->
		<Modal
			ref="librarySettingsModalRef"
			header="库设置"
			:closable="false"
			noblur
		>
			<div class="flex flex-col gap-4 w-[480px]">
				<div class="flex flex-col gap-1">
					<span class="text-sm font-medium text-primary">库名称</span>
					<Input
						v-model="librarySettingsName"
						type="text"
						wrapper-class="w-full"
						placeholder="留空则使用文件夹名"
					/>
					<span v-if="librarySettingsRenameError" class="text-sm text-danger">{{ librarySettingsRenameError }}</span>
				</div>
				<div class="flex flex-col gap-1">
					<span class="text-sm font-medium text-primary">库路径</span>
					<!-- Read-only: a library's path is its identity (instance IDs are
					     hashed from it), so it can only be added or removed, never edited. -->
					<Input
						:model-value="librarySettingsPath"
						type="text"
						wrapper-class="w-full"
						readonly
						disabled
					/>
					<span class="text-xs text-secondary">路径不可修改，如需更换位置请删除此库后重新添加。</span>
				</div>
				<div class="flex justify-between gap-2 pt-2">
					<Button
						color="danger"
						:disabled="libraries.length <= 1"
						:title="libraries.length <= 1 ? '至少需要保留一个库' : undefined"
						@click="removeLibrary"
					>
						删除库
					</Button>
					<div class="flex gap-2">
						<Button type="outlined" @click="closeLibrarySettingsModal">取消</Button>
						<Button type="colored" color="brand" :disabled="!librarySettingsName.trim()" @click="saveLibraryName">保存</Button>
					</div>
				</div>
			</div>
		</Modal>
		<div data-plugin-slot="home.top"></div>
		<RecentWorldsList
			v-if="showJumpBackInHome && recentInstances.length > 0"
			:recent-instances="recentInstances"
		/>
		<div data-plugin-slot="home.middle"></div>

        <h2 class="m-0 text-2xl font-semibold text-contrast">
            {{ formatMessage(messages.library) }}
        </h2>
        <!-- 库navtabs -->
        <div class="flex items-center gap-4">
            <NavTabs
                mode="local"
                :links="tabLinks"
                :active-index="activeTabIndex"
                reorderable
                :pinned-count="1"
                class="light-sense"
                @tab-click="handleTabClick"
                @reorder="handleTabReorder"
            />
            <NavButton
                class="bg-brand"
                style="--button-color: var(--color-brand) !important"
                :to="() => addLibraryModalRef?.show()"
            >
                <PlusIcon />
            </NavButton>
            <NavButton
                v-if="activeTab !== 'all'"
                :to="() => openLibrarySettings(activeTab)"
            >
                <CogIcon />
            </NavButton>
        </div>

		<!-- Library Section -->
		<LibrarySection :instances="instances" :library-path="activeTab === 'all' ? undefined : activeTab" />

		<div data-plugin-slot="home.bottom"></div>

		<ContextMenu ref="pageOptions" :label="formatMessage(messages.libraryActionsLabel)" />
	</div>
</template>
