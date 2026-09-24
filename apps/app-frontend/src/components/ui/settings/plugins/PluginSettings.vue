<script setup lang="ts">
import { TrashIcon, FolderOpenIcon, PlusIcon, TriangleAlertIcon, DownloadIcon } from '@modrinth/assets'
import {
	Button,
	defineMessages,
	injectNotificationManager,
	Toggle,
	useVIntl,
} from '@modrinth/ui'
import { open } from '@tauri-apps/plugin-dialog'
import { computed, onMounted, ref } from 'vue'

import { loadPlugins, unloadPlugin } from '@/plugin-host'
import {
	installPlugin,
	installPluginFromUrl,
	listPlugins,
	openPluginFolder,
	pluginCrashLog,
	setPluginEnabled,
	setPluginGranted,
	uninstallPlugin,
} from '@/plugin-host/ipc'
import { fetchStorePlugins, type StorePlugin } from '@/plugin-host/store'
import type { PluginSummary } from '@/plugin-host/types'

const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()

const messages = defineMessages({
	title: {
		id: 'app.settings.plugins.title',
		defaultMessage: 'Plugins',
	},
	description: {
		id: 'app.settings.plugins.description',
		defaultMessage:
			'Plugins extend the launcher. Each one declares what it needs, and anything you have not allowed simply will not work.',
	},
	install: {
		id: 'app.settings.plugins.install',
		defaultMessage: 'Install from folder',
	},
	openFolder: {
		id: 'app.settings.plugins.open-folder',
		defaultMessage: 'Open plugins folder',
	},
	loading: {
		id: 'app.settings.plugins.loading',
		defaultMessage: 'Loading plugins…',
	},
	empty: {
		id: 'app.settings.plugins.empty',
		defaultMessage: 'No plugins are installed.',
	},
	permissions: {
		id: 'app.settings.plugins.permissions',
		defaultMessage: 'Permissions',
	},
	highRisk: {
		id: 'app.settings.plugins.high-risk',
		defaultMessage: 'needs your approval',
	},
	uninstall: {
		id: 'app.settings.plugins.uninstall',
		defaultMessage: 'Uninstall',
	},
	crashLog: {
		id: 'app.settings.plugins.crash-log',
		defaultMessage: 'Show crash log',
	},
	appliesImmediately: {
		id: 'app.settings.plugins.applies-immediately',
		defaultMessage: 'Changes take effect immediately.',
	},
	broken: {
		id: 'app.settings.plugins.broken',
		defaultMessage: 'Could not load',
	},
	tabInstalled: {
		id: 'app.settings.plugins.tab-installed',
		defaultMessage: 'Installed',
	},
	tabStore: {
		id: 'app.settings.plugins.tab-store',
		defaultMessage: 'Store',
	},
	storeLoading: {
		id: 'app.settings.plugins.store-loading',
		defaultMessage: 'Loading the plugin store…',
	},
	storeError: {
		id: 'app.settings.plugins.store-error',
		defaultMessage: 'Could not load the plugin store.',
	},
	storeEmpty: {
		id: 'app.settings.plugins.store-empty',
		defaultMessage: 'The plugin store is empty.',
	},
	storeInstall: {
		id: 'app.settings.plugins.store-install',
		defaultMessage: 'Install',
	},
	storeInstalled: {
		id: 'app.settings.plugins.store-installed',
		defaultMessage: 'Installed',
	},
	storeRefresh: {
		id: 'app.settings.plugins.store-refresh',
		defaultMessage: 'Refresh',
	},
})

type Tab = 'installed' | 'store'
const tab = ref<Tab>('installed')

const plugins = ref<PluginSummary[]>([])
const loading = ref(true)
const busy = ref<string | null>(null)
const crashLogs = ref<Record<string, string>>({})

const storePlugins = ref<StorePlugin[]>([])
const storeLoading = ref(false)
const storeError = ref<string | null>(null)
const storeLoaded = ref(false)

const installedIds = computed(() => new Set(plugins.value.map((plugin) => plugin.id)))

async function reload() {
	try {
		plugins.value = await listPlugins()
	} catch (error) {
		handleError(error)
	} finally {
		loading.value = false
	}
}

onMounted(reload)

/** Apply a change to the running session without a restart. */
async function applyToSession(pluginId: string) {
	unloadPlugin(pluginId)
	await loadPlugins()
}

async function withBusy<T>(pluginId: string, work: () => Promise<T>): Promise<void> {
	if (busy.value) return
	busy.value = pluginId
	try {
		await work()
	} catch (error) {
		handleError(error)
	} finally {
		busy.value = null
	}
}

function setEnabled(plugin: PluginSummary, enabled: boolean) {
	return withBusy(plugin.id, async () => {
		await setPluginEnabled(plugin.id, enabled)
		await applyToSession(plugin.id)
		await reload()
	})
}

function setPermission(plugin: PluginSummary, permission: string, granted: boolean) {
	return withBusy(plugin.id, async () => {
		const next = granted
			? [...plugin.granted, permission]
			: plugin.granted.filter((entry) => entry !== permission)
		await setPluginGranted(plugin.id, next)
		await applyToSession(plugin.id)
		await reload()
	})
}

function onPermissionToggle(plugin: PluginSummary, permission: string, event: Event) {
	const input = event.target as HTMLInputElement
	void setPermission(plugin, permission, input.checked)
}

function uninstall(plugin: PluginSummary) {
	return withBusy(plugin.id, async () => {
		unloadPlugin(plugin.id)
		await uninstallPlugin(plugin.id, false)
		await reload()
	})
}

async function showCrashLog(plugin: PluginSummary) {
	try {
		const log = await pluginCrashLog(plugin.id)
		crashLogs.value[plugin.id] = log ?? '(no crash log)'
	} catch (error) {
		handleError(error)
	}
}

async function installFromFolder() {
	const picked = await open({ directory: true, multiple: false })
	if (typeof picked !== 'string') return
	try {
		await installPlugin(picked)
		await loadPlugins()
		await reload()
	} catch (error) {
		handleError(error)
	}
}

async function loadStore(force = false) {
	if (storeLoading.value) return
	if (storeLoaded.value && !force) return
	storeLoading.value = true
	storeError.value = null
	try {
		storePlugins.value = await fetchStorePlugins()
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

async function installFromStore(entry: StorePlugin) {
	if (busy.value) return
	busy.value = entry.id
	try {
		await installPluginFromUrl(entry.download, entry.repo)
		await loadPlugins()
		await reload()
		tab.value = 'installed'
	} catch (error) {
		handleError(error)
	} finally {
		busy.value = null
	}
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
				<Button @click="installFromFolder">
					<PlusIcon />
					{{ formatMessage(messages.install) }}
				</Button>
				<Button type="outlined" @click="openPluginFolder()">
					<FolderOpenIcon />
					{{ formatMessage(messages.openFolder) }}
				</Button>
			</div>

		<p v-if="loading" class="mt-6 mb-0 text-secondary">
			{{ formatMessage(messages.loading) }}
		</p>

		<p v-else-if="plugins.length === 0" class="mt-6 mb-0 text-secondary">
			{{ formatMessage(messages.empty) }}
		</p>

		<div v-else class="mt-6 flex flex-col gap-4">
			<article
				v-for="plugin in plugins"
				:key="plugin.id"
				class="rounded-xl border border-solid border-surface-5 bg-surface-3 p-4 flex flex-col gap-3"
			>
				<header class="flex items-start justify-between gap-4">
					<div class="min-w-0">
						<h3 class="m-0 text-lg font-semibold text-contrast">
							{{ plugin.manifest?.name ?? plugin.dir_name }}
						</h3>
						<p class="m-0 text-sm text-secondary">
							<span v-if="plugin.manifest?.version">v{{ plugin.manifest.version }}</span>
							<span v-if="plugin.manifest?.author"> · {{ plugin.manifest.author }}</span>
						</p>
					</div>
					<span class="inline-flex shrink-0">
						<Toggle
							:id="`plugin-enabled-${plugin.id}`"
							:model-value="plugin.enabled"
							:disabled="busy === plugin.id || Boolean(plugin.error)"
							@update:model-value="(value) => setEnabled(plugin, value)"
						/>
					</span>
				</header>

				<p v-if="plugin.manifest?.description" class="m-0 text-sm text-secondary">
					{{ plugin.manifest.description }}
				</p>

				<div
					v-if="plugin.error"
					class="flex items-start gap-2 text-sm text-danger"
				>
					<TriangleAlertIcon class="mt-0.5 shrink-0" />
					<span class="break-all">
						{{ formatMessage(messages.broken) }}: {{ plugin.error }}
					</span>
				</div>

				<div
					v-if="plugin.manifest?.permissions?.length"
					class="flex flex-col gap-2"
				>
					<h4 class="m-0 text-sm font-semibold text-contrast">
						{{ formatMessage(messages.permissions) }}
					</h4>
					<label
						v-for="permission in plugin.manifest.permissions"
						:key="permission"
						class="flex items-center gap-2 text-sm"
					>
						<input
							type="checkbox"
							:checked="plugin.granted.includes(permission)"
							:disabled="busy === plugin.id"
							@change="(event) => onPermissionToggle(plugin, permission, event)"
						/>
						<code class="text-xs">{{ permission }}</code>
						<span
							v-if="plugin.high_risk.includes(permission)"
							class="text-xs text-warning"
						>
							{{ formatMessage(messages.highRisk) }}
						</span>
					</label>
				</div>

				<div v-if="crashLogs[plugin.id]" class="flex flex-col gap-1">
					<h4 class="m-0 text-sm font-semibold text-contrast">
						{{ formatMessage(messages.crashLog) }}
					</h4>
					<pre
						class="m-0 max-h-48 overflow-auto rounded-lg bg-surface-1 p-2 text-xs whitespace-pre-wrap break-all"
					>{{ crashLogs[plugin.id] }}</pre>
				</div>

				<footer class="flex flex-wrap gap-2">
					<Button
						size="sm"
						type="outlined"
						:disabled="busy === plugin.id"
						@click="showCrashLog(plugin)"
					>
						{{ formatMessage(messages.crashLog) }}
					</Button>
					<Button
						size="sm"
						type="outlined"
						:disabled="busy === plugin.id"
						@click="openPluginFolder(plugin.id)"
					>
						<FolderOpenIcon />
					</Button>
					<Button
						size="sm"
						color="danger"
						:disabled="busy === plugin.id"
						@click="uninstall(plugin)"
					>
						<TrashIcon />
						{{ formatMessage(messages.uninstall) }}
					</Button>
				</footer>
			</article>
		</div>

			<p class="mt-4 mb-0 text-xs text-secondary">
				{{ formatMessage(messages.appliesImmediately) }}
			</p>
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
			<div
				v-else-if="storeError"
				class="mt-6 flex items-start gap-2 text-sm text-danger"
			>
				<TriangleAlertIcon class="mt-0.5 shrink-0" />
				<span class="break-all">
					{{ formatMessage(messages.storeError) }} {{ storeError }}
				</span>
			</div>
			<p v-else-if="storePlugins.length === 0" class="mt-6 mb-0 text-secondary">
				{{ formatMessage(messages.storeEmpty) }}
			</p>

			<div v-else class="mt-6 flex flex-col gap-4">
				<article
					v-for="entry in storePlugins"
					:key="entry.id"
					class="rounded-xl border border-solid border-surface-5 bg-surface-3 p-4 flex items-start justify-between gap-4"
				>
					<div class="min-w-0">
						<h3 class="m-0 text-lg font-semibold text-contrast">
							{{ entry.name }}
						</h3>
						<p class="m-0 text-sm text-secondary">
							<span v-if="entry.version">v{{ entry.version }}</span>
							<span v-if="entry.author"> · {{ entry.author }}</span>
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
	</section>
</template>
