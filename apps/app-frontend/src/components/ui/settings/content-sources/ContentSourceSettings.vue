<script setup lang="ts">
import {
	DownloadIcon,
	FolderOpenIcon,
	PlusIcon,
	ScaleIcon,
	SettingsIcon,
	TrashIcon,
	TriangleAlertIcon,
} from '@modrinth/assets'
import {
	Button,
	defineMessages,
	injectNotificationManager,
	SourceBadge,
	Toggle,
	useVIntl,
} from '@modrinth/ui'
import { open } from '@tauri-apps/plugin-dialog'
import { computed, onMounted, ref } from 'vue'

import {
	contentSourceAssetUrl,
	installSource,
	installSourceFromUrl,
	listSources,
	openSourceFolder,
	setSourceEnabled,
	sourceCrashLog,
	uninstallSource,
} from '@/content-sources/ipc'
import { loadContentSources } from '@/content-sources/loader'
import { MODRINTH_SOURCE_ID } from '@/content-sources/preferences'
import { contentSourceErrors } from '@/content-sources/registry'
import { fetchStoreSources, type StoreContentSource } from '@/content-sources/store'
import type { ContentType, SourceCapabilities, SourceSummary } from '@/content-sources/types'

import { toError } from '@/helpers/errors'
import ContentSourceMultiplierModal from './ContentSourceMultiplierModal.vue'
import SourceConfigModal from './SourceConfigModal.vue'

const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()

const CONTENT_TYPE_LABELS: Record<ContentType, string> = {
	mod: '模组',
	modpack: '整合包',
	resourcepack: '资源包',
	shader: '光影',
	datapack: '数据包',
	world: '世界',
	skin: '皮肤',
}

const CAPABILITY_LABELS: Record<keyof SourceCapabilities, string> = {
	search: '搜索',
	browse: '浏览',
	detail: '详情页',
	versions: '版本列表',
	resolve_download: '直接下载',
	update_check: '更新检查',
}

const messages = defineMessages({
	title: {
		id: 'app.settings.content-sources.title',
		defaultMessage: 'Content sources',
	},
	description: {
		id: 'app.settings.content-sources.description',
		defaultMessage:
			'Content sources are plugins that add browsable places to download content from. Discover uses every enabled source by default.',
	},
	tabInstalled: {
		id: 'app.settings.content-sources.tab-installed',
		defaultMessage: 'Installed',
	},
	tabStore: {
		id: 'app.settings.content-sources.tab-store',
		defaultMessage: 'Store',
	},
	install: {
		id: 'app.settings.content-sources.install',
		defaultMessage: 'Install from folder',
	},
	openFolder: {
		id: 'app.settings.content-sources.open-folder',
		defaultMessage: 'Open sources folder',
	},
	loading: {
		id: 'app.settings.content-sources.loading',
		defaultMessage: 'Loading content sources…',
	},
	empty: {
		id: 'app.settings.content-sources.empty',
		defaultMessage: 'No content sources are installed.',
	},
	emptyHint: {
		id: 'app.settings.content-sources.empty-hint',
		defaultMessage: 'Install one from the Store tab, or from a folder.',
	},
	types: {
		id: 'app.settings.content-sources.types',
		defaultMessage: 'Content types',
	},
	capabilities: {
		id: 'app.settings.content-sources.capabilities',
		defaultMessage: 'Capabilities',
	},
	hosts: {
		id: 'app.settings.content-sources.hosts',
		defaultMessage: 'Network access',
	},
	unsupported: {
		id: 'app.settings.content-sources.unsupported',
		defaultMessage: 'No capabilities available',
	},
	requiresLogin: {
		id: 'app.settings.content-sources.requires-login',
		defaultMessage: 'Requires sign-in',
	},
	broken: {
		id: 'app.settings.content-sources.broken',
		defaultMessage: 'Could not load',
	},
	crashLog: {
		id: 'app.settings.content-sources.crash-log',
		defaultMessage: 'Show crash log',
	},
	loadFailed: {
		id: 'app.settings.content-sources.load-failed',
		defaultMessage: 'Failed to start at runtime',
	},
	uninstall: {
		id: 'app.settings.content-sources.uninstall',
		defaultMessage: 'Uninstall',
	},
	storeLoading: {
		id: 'app.settings.content-sources.store-loading',
		defaultMessage: 'Loading the source store…',
	},
	storeError: {
		id: 'app.settings.content-sources.store-error',
		defaultMessage: 'Could not load the source store.',
	},
	storeEmpty: {
		id: 'app.settings.content-sources.store-empty',
		defaultMessage: 'The source store is empty.',
	},
	storeInstall: {
		id: 'app.settings.content-sources.store-install',
		defaultMessage: 'Install',
	},
	storeInstalled: {
		id: 'app.settings.content-sources.store-installed',
		defaultMessage: 'Installed',
	},
	storeRefresh: {
		id: 'app.settings.content-sources.store-refresh',
		defaultMessage: 'Refresh',
	},
	multipliers: {
		id: 'app.settings.content-sources.multipliers',
		defaultMessage: 'Source multipliers',
	},
	config: {
		id: 'app.settings.content-sources.config',
		defaultMessage: 'Settings',
	},
})

type Tab = 'installed' | 'store'
const tab = ref<Tab>('installed')

const sources = ref<SourceSummary[]>([])
const loading = ref(true)
const busy = ref<string | null>(null)
const crashLogs = ref<Record<string, string>>({})

const storeSources = ref<StoreContentSource[]>([])
const storeLoading = ref(false)
const storeError = ref<string | null>(null)
const storeLoaded = ref(false)

const installedIds = computed(() => new Set(sources.value.map((source) => source.id)))

const multiplierModal = ref<InstanceType<typeof ContentSourceMultiplierModal> | null>(null)

const configModal = ref<InstanceType<typeof SourceConfigModal> | null>(null)
const configTarget = ref<SourceSummary | null>(null)

function openConfig(source: SourceSummary) {
	configTarget.value = source
	configModal.value?.show()
}

/**
 * The source's own icon, resolved to a URL the webview may load — or null, and
 * the badge falls back to the source's first letter on its colour. Read from
 * the summary rather than `sourceAppearance`, so a source that is switched off
 * still shows the mark it ships.
 */
function sourceIconUrl(source: SourceSummary): string | null {
	const icon = source.manifest?.icon
	if (!icon || !source.path) return null
	return contentSourceAssetUrl(source.path, icon)
}

/** Every source Discover can pull from, the built-in one first. */
const multiplierSources = computed(() => [
	{ id: MODRINTH_SOURCE_ID, name: 'Modrinth' },
	...sources.value
		.filter((source) => source.manifest)
		.map((source) => ({
			id: source.id,
			name: source.manifest?.name ?? source.dir_name,
		})),
])

function openMultipliers() {
	multiplierModal.value?.show()
}

function typeLabel(type: ContentType): string {
	return CONTENT_TYPE_LABELS[type] ?? type
}

function enabledCapabilities(capabilities: SourceCapabilities): string[] {
	return (Object.keys(CAPABILITY_LABELS) as (keyof SourceCapabilities)[])
		.filter((key) => capabilities[key])
		.map((key) => CAPABILITY_LABELS[key])
}

/** A source's load failure from this session, if it threw while starting. */
function runtimeError(source: SourceSummary): string | null {
	return contentSourceErrors.value.get(source.id) ?? source.error
}

/** `handleError` wants an Error; a thrown value is not necessarily one. */
function reportError(error: unknown) {
	handleError(toError(error))
}

async function reload() {
	try {
		sources.value = await listSources()
	} catch (error) {
		reportError(error)
	} finally {
		loading.value = false
	}
}

onMounted(reload)

async function withBusy<T>(id: string, work: () => Promise<T>): Promise<void> {
	if (busy.value) return
	busy.value = id
	try {
		await work()
	} catch (error) {
		reportError(error)
	} finally {
		busy.value = null
	}
}

function toggleSource(source: SourceSummary, enabled: boolean) {
	return withBusy(source.id, async () => {
		await setSourceEnabled(source.id, enabled)
		await loadContentSources()
		await reload()
	})
}

function uninstall(source: SourceSummary) {
	return withBusy(source.id, async () => {
		await uninstallSource(source.id, false)
		await loadContentSources()
		await reload()
	})
}

function installFromFolder() {
	return withBusy('__install__', async () => {
		const picked = await open({ directory: true, multiple: false })
		if (typeof picked !== 'string') return
		await installSource(picked)
		await loadContentSources()
		await reload()
	})
}

async function showCrashLog(source: SourceSummary) {
	try {
		const log = (await sourceCrashLog(source.id)) ?? runtimeError(source)
		crashLogs.value[source.id] = log ?? '(no crash log)'
	} catch (error) {
		reportError(error)
	}
}

async function loadStore(force = false) {
	if (storeLoading.value) return
	if (storeLoaded.value && !force) return
	storeLoading.value = true
	storeError.value = null
	try {
		storeSources.value = await fetchStoreSources()
		storeLoaded.value = true
	} catch (error) {
		storeError.value = error instanceof Error ? error.message : String(error)
	} finally {
		storeLoading.value = false
	}
}

function showStore() {
	tab.value = 'store'
	void loadStore()
}

function installFromStore(entry: StoreContentSource) {
	return withBusy(entry.id, async () => {
		await installSourceFromUrl(entry.download, entry.repo)
		await loadContentSources()
		await reload()
		tab.value = 'installed'
	})
}
</script>

<template>
	<section>
		<h2 class="m-0 text-xl font-semibold text-contrast">
			{{ formatMessage(messages.title) }}
		</h2>
		<p class="mt-2 mb-0 text-secondary">
			{{ formatMessage(messages.description) }}
		</p>

		<div class="mt-4 flex gap-2 border-0 border-b border-solid border-surface-5">
			<button
				class="border-0 bg-transparent cursor-pointer px-3 py-2 text-sm font-semibold"
				:class="tab === 'installed' ? 'text-contrast border-b-2 border-solid border-brand' : 'text-secondary'"
				@click="tab = 'installed'"
			>
				{{ formatMessage(messages.tabInstalled) }}
			</button>
			<button
				class="border-0 bg-transparent cursor-pointer px-3 py-2 text-sm font-semibold"
				:class="tab === 'store' ? 'text-contrast border-b-2 border-solid border-brand' : 'text-secondary'"
				@click="showStore"
			>
				{{ formatMessage(messages.tabStore) }}
			</button>
		</div>

		<template v-if="tab === 'installed'">
			<div class="mt-4 flex flex-wrap gap-2">
				<Button :disabled="busy === '__install__'" @click="installFromFolder">
					<PlusIcon />
					{{ formatMessage(messages.install) }}
				</Button>
				<Button type="outlined" @click="openSourceFolder()">
					<FolderOpenIcon />
					{{ formatMessage(messages.openFolder) }}
				</Button>
				<Button type="outlined" @click="openMultipliers">
					<ScaleIcon />
					{{ formatMessage(messages.multipliers) }}
				</Button>
			</div>

			<p v-if="loading" class="mt-6 mb-0 text-secondary">
				{{ formatMessage(messages.loading) }}
			</p>

			<div v-else-if="sources.length === 0" class="mt-6 flex flex-col gap-2">
				<p class="m-0 text-secondary">{{ formatMessage(messages.empty) }}</p>
				<p class="m-0 text-sm text-secondary">{{ formatMessage(messages.emptyHint) }}</p>
			</div>

			<div v-else class="mt-6 flex flex-col gap-4">
				<article
					v-for="source in sources"
					:key="source.id"
					class="rounded-xl border border-solid border-surface-5 bg-surface-3 p-4 flex flex-col gap-3"
				>
					<header class="flex items-start justify-between gap-4">
						<div class="flex min-w-0 items-start gap-3">
							<SourceBadge
								:label="source.manifest?.name ?? source.dir_name"
								:color="source.manifest?.color ?? null"
								:icon-url="sourceIconUrl(source)"
								size="40px"
							/>
							<div class="min-w-0">
								<h3 class="m-0 text-lg font-semibold text-contrast">
									{{ source.manifest?.name ?? source.dir_name }}
								</h3>
								<p class="m-0 text-sm text-secondary">
									<span v-if="source.manifest?.version">v{{ source.manifest.version }}</span>
									<span v-if="source.manifest?.author"> · {{ source.manifest.author }}</span>
								</p>
							</div>
						</div>
						<span class="inline-flex shrink-0">
							<Toggle
								:id="`content-source-enabled-${source.id}`"
								:model-value="source.enabled"
								:disabled="busy === source.id || !source.manifest"
								@update:model-value="(value) => toggleSource(source, Boolean(value))"
							/>
						</span>
					</header>

					<p v-if="source.manifest?.description" class="m-0 text-sm text-secondary">
						{{ source.manifest.description }}
					</p>

					<div v-if="runtimeError(source)" class="flex flex-col gap-1">
						<div class="flex items-start gap-2 text-sm text-danger">
							<TriangleAlertIcon class="mt-0.5 shrink-0" />
							<span class="break-all">
								{{ source.error ? formatMessage(messages.broken) : formatMessage(messages.loadFailed) }}:
								{{ runtimeError(source) }}
							</span>
						</div>
						<Button size="sm" type="outlined" @click="showCrashLog(source)">
							{{ formatMessage(messages.crashLog) }}
						</Button>
						<pre
							v-if="crashLogs[source.id]"
							class="m-0 max-h-48 overflow-auto rounded-lg bg-surface-1 p-2 text-xs whitespace-pre-wrap break-all"
						>{{ crashLogs[source.id] }}</pre>
					</div>

					<div v-if="source.manifest?.content_types.length" class="flex flex-col gap-2">
						<h4 class="m-0 text-sm font-semibold text-contrast">
							{{ formatMessage(messages.types) }}
						</h4>
						<div class="flex flex-wrap gap-2">
							<span
								v-for="type in source.manifest.content_types"
								:key="type"
								class="inline-flex items-center rounded-full border border-solid border-surface-5 bg-surface-1 px-2.5 py-1 text-xs text-secondary"
							>
								{{ typeLabel(type) }}
							</span>
						</div>
					</div>

					<div class="flex flex-col gap-2">
						<h4 class="m-0 text-sm font-semibold text-contrast">
							{{ formatMessage(messages.capabilities) }}
						</h4>
						<div class="flex flex-wrap gap-2">
							<span
								v-for="capability in source.manifest ? enabledCapabilities(source.manifest.capabilities) : []"
								:key="capability"
								class="inline-flex items-center rounded-full bg-brand-highlight px-2.5 py-1 text-xs font-semibold text-brand"
							>
								{{ capability }}
							</span>
							<span
								v-if="source.manifest && enabledCapabilities(source.manifest.capabilities).length === 0"
								class="text-sm text-secondary"
							>
								{{ formatMessage(messages.unsupported) }}
							</span>
						</div>
					</div>

					<div v-if="source.manifest?.hosts.length" class="flex flex-col gap-2">
						<h4 class="m-0 text-sm font-semibold text-contrast">
							{{ formatMessage(messages.hosts) }}
						</h4>
						<div class="flex flex-wrap gap-2">
							<span
								v-for="host in source.manifest.hosts"
								:key="host"
								class="inline-flex items-center rounded-full border border-solid border-surface-5 bg-surface-1 px-2.5 py-1 text-xs text-secondary"
							>
								{{ host }}
							</span>
						</div>
					</div>

					<div v-if="source.manifest?.auth?.required" class="flex items-center gap-2 text-sm text-secondary">
						<TriangleAlertIcon class="shrink-0" />
						<span>{{ formatMessage(messages.requiresLogin) }}</span>
					</div>

					<footer class="flex flex-wrap items-center gap-2">
						<Button
							v-if="
								source.manifest &&
								(source.manifest.settings.length > 0 ||
									source.manifest.manual_entry.length > 0)
							"
							size="sm"
							:disabled="busy === source.id"
							@click="openConfig(source)"
						>
							<SettingsIcon />
							{{ formatMessage(messages.config) }}
						</Button>
						<Button
							size="sm"
							type="outlined"
							:disabled="busy === source.id"
							@click="openSourceFolder(source.id)"
						>
							<FolderOpenIcon />
						</Button>
						<Button
							size="sm"
							color="red"
							:disabled="busy === source.id"
							@click="uninstall(source)"
						>
							<TrashIcon />
							{{ formatMessage(messages.uninstall) }}
						</Button>
					</footer>
				</article>
			</div>
		</template>

		<template v-else>
			<div class="mt-4 flex justify-end">
				<Button type="outlined" :disabled="storeLoading" @click="loadStore(true)">
					{{ formatMessage(messages.storeRefresh) }}
				</Button>
			</div>

			<p v-if="storeLoading" class="mt-6 mb-0 text-secondary">
				{{ formatMessage(messages.storeLoading) }}
			</p>
			<div v-else-if="storeError" class="mt-6 flex items-start gap-2 text-sm text-danger">
				<TriangleAlertIcon class="mt-0.5 shrink-0" />
				<span class="break-all">{{ formatMessage(messages.storeError) }} {{ storeError }}</span>
			</div>
			<p v-else-if="storeSources.length === 0" class="mt-6 mb-0 text-secondary">
				{{ formatMessage(messages.storeEmpty) }}
			</p>

			<div v-else class="mt-6 flex flex-col gap-4">
				<article
					v-for="entry in storeSources"
					:key="entry.id"
					class="rounded-xl border border-solid border-surface-5 bg-surface-3 p-4 flex items-start justify-between gap-4"
				>
					<div class="min-w-0">
						<h3 class="m-0 text-lg font-semibold text-contrast">
							{{ entry.name }}
						</h3>
						<p v-if="entry.author" class="m-0 text-sm text-secondary">
							{{ entry.author }}
						</p>
						<p v-if="entry.description" class="mt-1 mb-0 text-sm text-secondary">
							{{ entry.description }}
						</p>
					</div>
					<Button
						v-if="installedIds.has(entry.id)"
						size="sm"
						disabled
					>
						{{ formatMessage(messages.storeInstalled) }}
					</Button>
					<Button
						v-else
						size="sm"
						color="brand"
						:disabled="busy === entry.id"
						@click="installFromStore(entry)"
					>
						<DownloadIcon />
						{{ formatMessage(messages.storeInstall) }}
					</Button>
				</article>
			</div>
		</template>

		<ContentSourceMultiplierModal ref="multiplierModal" :sources="multiplierSources" />
		<SourceConfigModal ref="configModal" :source="configTarget" />
	</section>
</template>
