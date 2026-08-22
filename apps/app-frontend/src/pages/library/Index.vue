<script setup lang="ts">
import {BoxIcon, FolderSearchIcon, LibraryIcon, PlusIcon, SettingsIcon} from '@modrinth/assets'
import {
    ButtonStyled, commonMessages,
    DropdownSelect,
    injectNotificationManager,
    NavTabs,
    NewModal as Modal,
    StyledInput,
} from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { inject, onUnmounted, ref, shallowRef, watch, watchEffect } from 'vue'
import { useRoute } from 'vue-router'

import { NewInstanceImage } from '@/assets/icons'
import { instance_listener } from '@/helpers/events.js'
import { list } from '@/helpers/instance'
import type { InstanceFormat } from '@/helpers/library'
import { library_default_path, library_list } from '@/helpers/library'
import { useRootBreadcrumb } from '@/providers/breadcrumbs'
import NavButton from "@/components/ui/NavButton.vue";

const { handleError } = injectNotificationManager()
const showCreationModal = inject('showCreationModal')
const route = useRoute()

useRootBreadcrumb({
	slot: 'root',
	id: 'library',
	label: 'Library',
	to: '/library',
	visual: { type: 'icon', component: LibraryIcon },
})

const instances = shallowRef(await list().catch(handleError))
const offline = ref(!navigator.onLine)
window.addEventListener('offline', () => {
	offline.value = true
})
window.addEventListener('online', () => {
	offline.value = false
})

const unlistenInstance = await instance_listener(async () => {
	instances.value = await list(activeTab.value === 'all' ? undefined : activeTab.value).catch(handleError)
})
onUnmounted(() => {
	unlistenInstance()
})

const libraries = shallowRef<{ name: string; path: string; type: string }[]>([])
let defaultLibraryPath: string | null = null
const loadLibraries = async () => {
	try {
		const [config, defaultPath] = await Promise.all([
			library_list(),
			library_default_path().catch((e) => { console.log('[debug] library_default_path error:', e); return null; }),
		])
		console.log('[debug] defaultPath raw:', JSON.stringify(defaultPath))
		console.log('[debug] libraries raw:', JSON.stringify(config.libraries))
		libraries.value = config.libraries
		defaultLibraryPath = defaultPath
	} catch (e) {
		handleError(e as Error)
	}
}
loadLibraries()

const activeTab = ref('all')

const tabLinks = shallowRef([
	{ label: '全部实例', href: `/library` },
])

const libDisplayLabel = (lib: { name: string; path: string }): string => {
	if (lib.name) return lib.name
	// Check against the default library path (normalise both sides)
	if (defaultLibraryPath) {
		const normDefault = defaultLibraryPath.replace(/\\/g, '/').toLowerCase().replace(/\/+$/, '')
		const normLib = lib.path.replace(/\\/g, '/').toLowerCase().replace(/\/+$/, '')
		console.log('[debug] libDisplayLabel: defaultPath=', JSON.stringify(defaultLibraryPath), 'normDefault=', JSON.stringify(normDefault))
		console.log('[debug] libDisplayLabel: lib.path=', JSON.stringify(lib.path), 'normLib=', JSON.stringify(normLib))
		console.log('[debug] libDisplayLabel: match1=', normLib === normDefault, 'match2=', normLib === normDefault + '/profiles')
		if (normLib === normDefault || normLib === normDefault + '/profiles') {
			return '默认库'
		}
	}
	const folderName = lib.path
		.split('/')
		.pop()
		?.split('\\')
		.pop()
	|| lib.path
	return folderName || lib.path
}

const updateTabs = () => {
	const tabs = [{ label: '全部实例', href: '/library' }]
	for (const lib of libraries.value) {
		const label = libDisplayLabel(lib)
		tabs.push({ label, href: `/library/lib/${encodeURIComponent(lib.path)}` })
	}
	tabLinks.value = tabs
}

watchEffect(() => {
	updateTabs()
	// Sync active tab
	const currentPath = route.path
	if (currentPath.startsWith('/library/lib/')) {
		const libPath = decodeURIComponent(currentPath.replace('/library/lib/', ''))
		if (libraries.value.some((l) => l.path === libPath)) {
			activeTab.value = libPath
			return
		}
		// Don't reset to 'all' while libraries are still loading;
		// let the watcher fire once they're populated.
		if (libraries.value.length > 0) {
			activeTab.value = 'all'
		}
	} else if (libraries.value.length > 0) {
		activeTab.value = 'all'
	}
})

watch(activeTab, async (tab) => {
	instances.value = await list(tab === 'all' ? undefined : tab).catch(handleError)
})

const addLibraryPath = ref('')
const addLibraryFormat = ref<InstanceFormat>('modrinth')
const addLibraryName = ref('')
const addLibraryModalRef = ref<InstanceType<typeof Modal> | null>(null)

defineExpose({
	show: () => addLibraryModalRef.value?.show(),
	hide: () => addLibraryModalRef.value?.hide(),
})

async function pickFolder() {
	const result = await open({ directory: true, multiple: false })
	if (result && typeof result === 'string') {
		addLibraryPath.value = result
	}
}

async function addLibrary() {
	if (!addLibraryPath.value.trim()) return
	try {
		await invoke('plugin:instance|library_add', {
			path: addLibraryPath.value.trim(),
			format: addLibraryFormat.value,
			name: addLibraryName.value.trim() || undefined,
		})
		closeAddLibraryModal()
		addLibraryPath.value = ''
		addLibraryName.value = ''
		await loadLibraries()
		instances.value = await list(activeTab.value === 'all' ? undefined : activeTab.value).catch(handleError)
	} catch (e) {
		handleError(e as Error)
	}
}

function closeAddLibraryModal() {
	addLibraryModalRef.value?.hide()
}
</script>

<template>
	<div class="p-6 flex flex-col gap-3">
		<h1 class="m-0 text-2xl hidden">库</h1>
		<div class="flex items-center gap-4">
			<NavTabs
				:links="tabLinks"
				:active-href="route.path"
			/>
<!--			<button
				class="btn btn-brand rounded-full w-8 h-8 p-0 flex items-center justify-center transition-all hover:scale-105"
				:aria-label="$t('common.add')"
				@click="addLibraryModalRef?.show()"
			>
				<PlusIcon class="size-4" />
			</button>-->
<!--            <NavButton
                v-tooltip.right="formatMessage(commonMessages.settingsLabel)"
                :to="() => appSettingsModal?.show()"
            >
                <SettingsIcon />
            </NavButton>-->
            <NavButton
                :to="() => addLibraryModalRef?.show()"
            >
                <PlusIcon />
            </NavButton>

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
                        id="appDir"
                        v-model="addLibraryName"
                        placeholder="留空则使用文件夹名"
                        type="text"
                        wrapper-class="w-full"
                    >
                    </StyledInput>
				</div>
                <h2 class="m-0 text-lg font-semibold text-contrast">
                    库位置
                </h2>
                <StyledInput
                    id="appDir"
                    v-model="addLibraryPath"
                    placeholder="选择文件夹或输入路径"
                    :icon="BoxIcon"
                    type="text"
                    wrapper-class="w-full"
                >
                    <template #right>
                        <ButtonStyled circular>
                            <button
                                v-tooltip="'浏览文件夹'"
                                :aria-label="'浏览文件夹'"
                                class="ml-1.5"
                                @click="pickFolder"
                            >
                                <FolderSearchIcon aria-hidden="true" />
                            </button>
                        </ButtonStyled>
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
					<ButtonStyled variant="secondary" @click="closeAddLibraryModal">
						取消
					</ButtonStyled>
					<ButtonStyled
						color="brand"
						:disabled="!addLibraryPath.trim()"
						@click="addLibrary"
					>
						添加
					</ButtonStyled>
				</div>
			</div>
		</Modal>

		<template v-if="instances && instances.length > 0">
			<RouterView
				v-if="route.path.startsWith('/library')"
				:instances="instances"
			/>
		</template>
		<div v-else class="no-instance">
			<div class="icon">
				<NewInstanceImage />
			</div>
			<h3>未找到实例</h3>
			<ButtonStyled color="brand">
				<button :disabled="offline" @click="showCreationModal?.()">
					<PlusIcon />
					添加新的版本
				</button>
			</ButtonStyled>
		</div>
	</div>
</template>

<style lang="scss" scoped>
.no-instance {
	display: flex;
	flex-direction: column;
	align-items: center;
	justify-content: center;
	height: 100%;
	gap: var(--gap-md);

	p,
	h3 {
		margin: 0;
	}

	.icon {
		svg {
			width: 10rem;
			height: 10rem;
		}
	}
}

.fade-enter-active,
.fade-leave-active {
	transition: opacity 0.2s ease;
}
.fade-enter-from,
.fade-leave-to {
	opacity: 0;
}
</style>
