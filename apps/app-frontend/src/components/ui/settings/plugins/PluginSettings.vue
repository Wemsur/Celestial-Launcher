<script setup lang="ts">
import { TrashIcon, FolderOpenIcon, PlusIcon, TriangleAlertIcon, DownloadIcon, SettingsIcon } from '@modrinth/assets'
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
	pluginGetHotReload,
	pluginLatestRelease,
	pluginSetHotReload,
	setPluginEnabled,
	setPluginGranted,
	uninstallPlugin,
	updatePluginFromUrl,
} from '@/plugin-host/ipc'
import { fetchStorePlugins, type StorePlugin } from '@/plugin-host/store'
import type { PluginSummary } from '@/plugin-host/types'
import PluginSettingsModal from './PluginSettingsModal.vue'

const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()

const settingsModal = ref<InstanceType<typeof PluginSettingsModal> | null>(null)
const settingsTarget = ref<PluginSummary | null>(null)

/**
 * Human labels for permission strings. A permission is `kind` or `kind:scope`;
 * the kind is translated and the scope kept verbatim (a slot/host/event name).
 */
const PERMISSION_KIND_LABELS: Record<string, string> = {
	style: '显示样式',
	storage: '本地存储',
	slot: '界面插槽',
	route: '自定义页面',
	event: '事件监听',
	hostapi: '启动器接口',
	region: '界面区域',
	network: '网络访问',
	sidecar: '运行本地程序',
	lan: '局域网联机',
}

function permissionLabel(permission: string): string {
	const [kind, scope] = permission.split(':')
	const label = PERMISSION_KIND_LABELS[kind] ?? kind
	return scope ? `${label}：${scope}` : label
}

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
		defaultMessage: 'click to approve',
	},
	uninstall: {
		id: 'app.settings.plugins.uninstall',
		defaultMessage: 'Uninstall',
	},
	settings: {
		id: 'app.settings.plugins.settings',
		defaultMessage: 'Settings',
	},
	crashLog: {
		id: 'app.settings.plugins.crash-log',
		defaultMessage: 'Show crash log',
	},
	appliesImmediately: {
		id: 'app.settings.plugins.applies-immediately',
		defaultMessage: 'Changes take effect immediately.',
	},
	appliesAfterRestart: {
		id: 'app.settings.plugins.applies-after-restart',
		defaultMessage: 'Changes take effect after you restart the launcher.',
	},
	hotReload: {
		id: 'app.settings.plugins.hot-reload',
		defaultMessage: 'Hot reload',
	},
	hotReloadDescription: {
		id: 'app.settings.plugins.hot-reload.description',
		defaultMessage:
			'Apply enable, disable, and permission changes to the running launcher immediately. When off, changes take effect after a restart.',
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
	hasUpdate: {
		id: 'app.settings.plugins.has-update',
		defaultMessage: 'Update available',
	},
	update: {
		id: 'app.settings.plugins.update',
		defaultMessage: 'Update',
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

const hotReload = ref(true)

const installedIds = computed(() => new Set(plugins.value.map((plugin) => plugin.id)))

/** Numeric, dotted-version comparison; true when `candidate` is newer than `current`. */
function isNewerVersion(candidate: string, current: string): boolean {
	const parse = (value: string) =>
		value
			.split(/[.\-+ _]/)
			.map((part) => Number.parseInt(part, 10))
			.filter((part) => !Number.isNaN(part))
	const a = parse(candidate)
	const b = parse(current)
	for (let i = 0; i < Math.max(a.length, b.length); i++) {
		const x = a[i] ?? 0
		const y = b[i] ?? 0
		if (x !== y) return x > y
	}
	return false
}

/**
 * Latest release tag per `owner/name`, filled lazily from the store index.
 * A session memo: empty string marks "checked, no release" so a repo is not
 * queried twice, and it is not persisted — a restart re-checks.
 */
const latestTags = ref<Record<string, string>>({})
const tagsInFlight = new Set<string>()

/** Query one repo's latest release tag once per session. Best-effort, silent. */
async function ensureLatestTag(repo: string) {
	if (repo in latestTags.value || tagsInFlight.has(repo)) return
	tagsInFlight.add(repo)
	try {
		const tag = await pluginLatestRelease(repo)
		latestTags.value[repo] = tag ?? ''
	} catch {
		// Update detection is best-effort; a failed check just shows no version.
	} finally {
		tagsInFlight.delete(repo)
	}
}

/** The known latest tag for a store entry's repo, if one has been fetched. */
function latestTag(entry: StorePlugin): string | undefined {
	const tag = entry.github ? latestTags.value[entry.github] : undefined
	return tag ? tag : undefined
}

/**
 * For each installed plugin, the store entry offering a newer release, keyed by
 * plugin id. The newer version is decided by comparing the plugin's local
 * manifest version against its repo's latest release tag.
 */
const updates = computed(() => {
	const byId = new Map(storePlugins.value.map((entry) => [entry.id, entry]))
	const result = new Map<string, { entry: StorePlugin; tag: string }>()
	for (const plugin of plugins.value) {
		const installed = plugin.manifest?.version
		const entry = byId.get(plugin.id)
		const tag = entry ? latestTag(entry) : undefined
		if (installed && entry && tag && isNewerVersion(tag, installed)) {
			result.set(plugin.id, { entry, tag })
		}
	}
	return result
})

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
onMounted(() => {
	// Fetch the store index in the background so the installed tab can flag
	// plugins that have a newer version available. Failures stay silent here;
	// they only surface when the user opens the store tab.
	void loadStore()
})
onMounted(async () => {
	try {
		hotReload.value = await pluginGetHotReload()
	} catch (error) {
		handleError(error)
	}
})

async function setHotReload(enabled: boolean) {
	hotReload.value = enabled
	try {
		await pluginSetHotReload(enabled)
	} catch (error) {
		handleError(error)
	}
}

/** Apply a change to the running session without a restart, when hot reload is on. */
async function applyToSession(pluginId: string) {
	if (!hotReload.value) return
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

function onPermissionToggle(plugin: PluginSummary, permission: string) {
	void setPermission(plugin, permission, !plugin.granted.includes(permission))
}

function openSettings(plugin: PluginSummary) {
	settingsTarget.value = plugin
	settingsModal.value?.show()
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
		// Kick off a latest-release check per repo so both tabs can show the
		// current version and flag updates. Memoised, so this is cheap on reload.
		for (const entry of storePlugins.value) {
			if (entry.github) void ensureLatestTag(entry.github)
		}
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

function updateFromStore(plugin: PluginSummary, entry: StorePlugin) {
	return withBusy(plugin.id, async () => {
		unloadPlugin(plugin.id)
		await updatePluginFromUrl(entry.download, entry.repo)
		await loadPlugins()
		await reload()
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
						<span
							v-if="updates.get(plugin.id)"
							class="mt-1 inline-flex items-center gap-1 rounded-full bg-brand-highlight px-2 py-0.5 text-xs font-semibold text-brand"
						>
							{{ formatMessage(messages.hasUpdate) }} · {{ updates.get(plugin.id)?.tag }}
						</span>
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
					<div class="flex flex-wrap gap-2">
						<button
							v-for="permission in plugin.manifest.permissions"
							:key="permission"
							type="button"
							:disabled="busy === plugin.id"
							class="plugin-permission-tag"
							:class="{
								'plugin-permission-tag--granted': plugin.granted.includes(permission),
								'plugin-permission-tag--high-risk': plugin.high_risk.includes(permission),
							}"
							:title="permission"
							@click="onPermissionToggle(plugin, permission)"
						>
							<span>{{ permissionLabel(permission) }}</span>
							<span
								v-if="plugin.high_risk.includes(permission) && !plugin.granted.includes(permission)"
								class="text-xs opacity-80"
							>
								· {{ formatMessage(messages.highRisk) }}
							</span>
						</button>
					</div>
				</div>

				<div v-if="crashLogs[plugin.id]" class="flex flex-col gap-1">
					<h4 class="m-0 text-sm font-semibold text-contrast">
						{{ formatMessage(messages.crashLog) }}
					</h4>
					<pre
						class="m-0 max-h-48 overflow-auto rounded-lg bg-surface-1 p-2 text-xs whitespace-pre-wrap break-all"
					>{{ crashLogs[plugin.id] }}</pre>
				</div>

				<footer class="flex flex-wrap items-center gap-2">
					<Button
						v-if="updates.get(plugin.id)"
						size="sm"
						color="brand"
						:disabled="busy === plugin.id"
						@click="updateFromStore(plugin, updates.get(plugin.id)!.entry)"
					>
						<DownloadIcon />
						{{ formatMessage(messages.update) }}
					</Button>
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
					<Button
						size="sm"
						class="ml-auto"
						:disabled="busy === plugin.id || Boolean(plugin.error)"
						@click="openSettings(plugin)"
					>
						<SettingsIcon />
						{{ formatMessage(messages.settings) }}
					</Button>
				</footer>
			</article>
		</div>

			<div
				class="mt-4 flex items-center justify-between gap-4 border-0 border-t border-solid border-surface-5 pt-4"
			>
				<div class="flex flex-col gap-1 min-w-0">
					<span class="text-sm font-semibold text-contrast">
						{{ formatMessage(messages.hotReload) }}
					</span>
					<span class="text-xs text-secondary">
						{{ formatMessage(messages.hotReloadDescription) }}
					</span>
				</div>
				<span class="inline-flex shrink-0">
					<Toggle
						id="plugin-hot-reload"
						:model-value="hotReload"
						@update:model-value="setHotReload"
					/>
				</span>
			</div>

			<p class="mt-4 mb-0 text-xs text-secondary">
				{{
					formatMessage(hotReload ? messages.appliesImmediately : messages.appliesAfterRestart)
				}}
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
							<span v-if="latestTag(entry)">{{ latestTag(entry) }}</span>
							<span v-if="entry.author"> · {{ entry.author }}</span>
						</p>
						<p v-if="entry.description" class="mt-1 mb-0 text-sm text-secondary">
							{{ entry.description }}
						</p>
					</div>
					<Button
						v-if="updates.get(entry.id)"
						size="sm"
						color="brand"
						:disabled="busy === entry.id"
						@click="updateFromStore(plugins.find((plugin) => plugin.id === entry.id)!, entry)"
					>
						<DownloadIcon />
						{{ formatMessage(messages.update) }}
					</Button>
					<Button
						v-else-if="installedIds.has(entry.id)"
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

		<PluginSettingsModal ref="settingsModal" :plugin="settingsTarget" />
	</section>
</template>

<style scoped>
.plugin-permission-tag {
	display: inline-flex;
	align-items: center;
	gap: 0.25rem;
	padding: 0.25rem 0.625rem;
	line-height: 1;
	border-radius: 9999px;
	border: 1px solid var(--color-button-bg);
	background: var(--color-button-bg);
	color: var(--color-secondary);
	font-size: 0.8125rem;
	cursor: pointer;
	transition: transform 0.1s ease;
}
.plugin-permission-tag:active {
	transform: scale(0.95);
}
.plugin-permission-tag:disabled {
	cursor: default;
	opacity: 0.6;
}
.plugin-permission-tag--granted {
	border-color: var(--color-brand);
	color: var(--color-brand);
}
.plugin-permission-tag--high-risk:not(.plugin-permission-tag--granted) {
	border-color: var(--color-orange);
	color: var(--color-orange);
}
.plugin-permission-tag--high-risk.plugin-permission-tag--granted {
	border-color: #1bd96a;
	color: #1bd96a;
}
</style>
