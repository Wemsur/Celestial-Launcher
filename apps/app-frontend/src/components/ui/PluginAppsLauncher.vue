<template>
	<template v-if="pages.length > 0">
		<button
			v-for="page in pinnedPages"
			:key="keyOf(page)"
			v-tooltip.right="page.title"
			class="apps-nav-btn"
			:class="isActive(page) ? 'apps-nav-btn-active' : 'text-primary'"
			@click="openPage(page)"
		>
			<span class="apps-icon" v-html="page.icon || FALLBACK_ICON"></span>
		</button>

		<button
			ref="triggerEl"
			v-tooltip.right="formatMessage(messages.title)"
			class="apps-nav-btn text-2xl"
			:class="open ? 'apps-nav-btn-open' : 'text-primary'"
			@mouseenter="onTriggerEnter"
			@mouseleave="onLeave"
			@click="toggle"
		>
			<LayoutGridIcon />
		</button>

		<Teleport to="body">
			<div
				v-if="open"
				ref="panelEl"
				class="plugin-apps-panel bg-bg-raised"
				:style="panelStyle"
				@mouseenter="onPanelEnter"
				@mouseleave="onLeave"
			>
				<div class="apps-header">
					<h2 class="apps-title">{{ formatMessage(messages.title) }}</h2>
					<button v-if="!manage" class="apps-manage-btn" @click="enterManage">
						<SettingsIcon class="size-4" />
						{{ formatMessage(messages.manage) }}
					</button>
				</div>
				<template v-if="manage">
					<p class="apps-section-label">{{ formatMessage(messages.added) }}</p>
					<div v-if="draftPinned.length > 0" class="apps-grid">
						<div v-for="page in draftPinned" :key="keyOf(page)" class="apps-tile">
							<button
								class="apps-badge apps-badge-remove"
								:aria-label="formatMessage(messages.remove)"
								@click="toggleDraft(page)"
							>
								<MinusIcon />
							</button>
							<span class="apps-icon-lg" v-html="page.icon || FALLBACK_ICON"></span>
							<span class="apps-tile-label">{{ page.title }}</span>
						</div>
					</div>
					<p v-else class="apps-empty">{{ formatMessage(messages.noneAdded) }}</p>

					<p class="apps-section-label">{{ formatMessage(messages.notAdded) }}</p>
					<div v-if="draftUnpinned.length > 0" class="apps-grid">
						<div v-for="page in draftUnpinned" :key="keyOf(page)" class="apps-tile">
							<button
								class="apps-badge apps-badge-add"
								:aria-label="formatMessage(messages.add)"
								@click="toggleDraft(page)"
							>
								<PlusIcon />
							</button>
							<span class="apps-icon-lg" v-html="page.icon || FALLBACK_ICON"></span>
							<span class="apps-tile-label">{{ page.title }}</span>
						</div>
					</div>
					<p v-else class="apps-empty">{{ formatMessage(messages.allAdded) }}</p>

					<div class="apps-footer">
						<Button type="outlined" @click="cancelManage">
							{{ formatMessage(messages.cancel) }}
						</Button>
						<Button type="colored" color="brand" @click="confirmManage">
							{{ formatMessage(messages.confirm) }}
						</Button>
					</div>
				</template>
				<template v-else>
					<div v-if="collapsedPages.length > 0" class="apps-grid">
						<button
							v-for="page in collapsedPages"
							:key="keyOf(page)"
							class="apps-tile apps-tile-click"
							@click="openPage(page)"
						>
							<span class="apps-icon-lg" v-html="page.icon || FALLBACK_ICON"></span>
							<span class="apps-tile-label">{{ page.title }}</span>
						</button>
					</div>
					<p v-else class="apps-empty">{{ formatMessage(messages.allPinned) }}</p>
				</template>
			</div>
		</Teleport>
	</template>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LayoutGridIcon, MinusIcon, PlusIcon, SettingsIcon } from '@modrinth/assets'
import { Button, defineMessages, useVIntl } from '@modrinth/ui'

import { sidebarPages, type SidebarPageEntry } from '@/plugin-host'
import { listPlugins, setPluginPinnedPages } from '@/plugin-host/ipc'

const { formatMessage } = useVIntl()
const route = useRoute()
const router = useRouter()

const FALLBACK_ICON =
	'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/></svg>'

// The reactive registry the loader fills as plugins register sidebar pages.
const pages = sidebarPages

// Pinned state, keyed by plugin id + route path. Seeded from the persisted
// `pinned_pages` on each plugin and re-synced after a manage session commits.
const pinnedKeys = reactive(new Set<string>())
// The staged copy edited while the manage panel is open; committed on confirm.
const draftKeys = reactive(new Set<string>())

const open = ref(false)
const manage = ref(false)

const keyOf = (page: SidebarPageEntry) => `${page.pluginId}\n${page.path}`

const pinnedPages = computed(() => pages.filter((page) => pinnedKeys.has(keyOf(page))))
const collapsedPages = computed(() => pages.filter((page) => !pinnedKeys.has(keyOf(page))))
const draftPinned = computed(() => pages.filter((page) => draftKeys.has(keyOf(page))))
const draftUnpinned = computed(() => pages.filter((page) => !draftKeys.has(keyOf(page))))

const isActive = (page: SidebarPageEntry) => route.path === page.path

const messages = defineMessages({
	title: { id: 'app.plugin-apps.title', defaultMessage: 'More features' },
	manage: { id: 'app.plugin-apps.manage', defaultMessage: 'Manage' },
	added: { id: 'app.plugin-apps.added', defaultMessage: 'Added features' },
	notAdded: { id: 'app.plugin-apps.not-added', defaultMessage: 'Available features' },
	add: { id: 'app.plugin-apps.add', defaultMessage: 'Add to sidebar' },
	remove: { id: 'app.plugin-apps.remove', defaultMessage: 'Remove from sidebar' },
	cancel: { id: 'app.plugin-apps.cancel', defaultMessage: 'Cancel' },
	confirm: { id: 'app.plugin-apps.confirm', defaultMessage: 'Confirm' },
	noneAdded: { id: 'app.plugin-apps.none-added', defaultMessage: 'No features pinned yet.' },
	allAdded: { id: 'app.plugin-apps.all-added', defaultMessage: 'Every feature is pinned.' },
	allPinned: { id: 'app.plugin-apps.all-pinned', defaultMessage: 'Every feature is on the sidebar.' },
})

const triggerEl = ref<HTMLElement | null>(null)
const panelEl = ref<HTMLElement | null>(null)
const panelPos = reactive({ top: 0, left: 0, maxHeight: 0 })
const panelStyle = computed(() => ({
	top: `${panelPos.top}px`,
	left: `${panelPos.left}px`,
	maxHeight: `${panelPos.maxHeight}px`,
}))

function updatePosition() {
	const el = triggerEl.value
	if (!el) return
	const rect = el.getBoundingClientRect()
	panelPos.left = rect.right + 8
	panelPos.top = Math.max(8, rect.top)
	panelPos.maxHeight = window.innerHeight - panelPos.top - 16
}

let closeTimer: ReturnType<typeof setTimeout> | null = null
function cancelClose() {
	if (closeTimer) {
		clearTimeout(closeTimer)
		closeTimer = null
	}
}
function openNow() {
	cancelClose()
	open.value = true
	requestAnimationFrame(updatePosition)
}
function onTriggerEnter() {
	openNow()
}
function onPanelEnter() {
	cancelClose()
}
function onLeave() {
	// Manage mode is a deliberate session: it stays open until confirmed,
	// cancelled, or dismissed with Escape, even when the pointer leaves.
	if (manage.value) return
	cancelClose()
	closeTimer = setTimeout(() => {
		open.value = false
	}, 150)
}
function toggle() {
	if (open.value) {
		if (manage.value) return
		cancelClose()
		open.value = false
	} else {
		openNow()
	}
}
function openPage(page: SidebarPageEntry) {
	void router.push(page.path)
	cancelClose()
	open.value = false
}

function enterManage() {
	draftKeys.clear()
	pinnedKeys.forEach((key) => draftKeys.add(key))
	manage.value = true
	requestAnimationFrame(updatePosition)
}
function cancelManage() {
	manage.value = false
	requestAnimationFrame(updatePosition)
}
function toggleDraft(page: SidebarPageEntry) {
	const key = keyOf(page)
	if (draftKeys.has(key)) draftKeys.delete(key)
	else draftKeys.add(key)
}
async function confirmManage() {
	// Group the drafted pin state by plugin and persist only the plugins whose
	// set of pinned pages actually changed.
	const nextByPlugin = new Map<string, string[]>()
	for (const page of pages) {
		if (!nextByPlugin.has(page.pluginId)) nextByPlugin.set(page.pluginId, [])
		if (draftKeys.has(keyOf(page))) nextByPlugin.get(page.pluginId)!.push(page.path)
	}
	const prevByPlugin = new Map<string, string[]>()
	for (const page of pages) {
		if (!prevByPlugin.has(page.pluginId)) prevByPlugin.set(page.pluginId, [])
		if (pinnedKeys.has(keyOf(page))) prevByPlugin.get(page.pluginId)!.push(page.path)
	}
	const sameSet = (a: string[], b: string[]) =>
		a.length === b.length && a.every((path) => b.includes(path))
	await Promise.all(
		[...nextByPlugin].map(([pluginId, paths]) => {
			if (sameSet(paths, prevByPlugin.get(pluginId) ?? [])) return undefined
			return setPluginPinnedPages(pluginId, paths).catch(() => {})
		}),
	)
	pinnedKeys.clear()
	draftKeys.forEach((key) => pinnedKeys.add(key))
	manage.value = false
	requestAnimationFrame(updatePosition)
}

async function refreshPinned() {
	const summaries = await listPlugins().catch(() => [])
	const next = new Set<string>()
	for (const summary of summaries) {
		for (const path of summary.pinned_pages ?? []) {
			next.add(`${summary.id}\n${path}`)
		}
	}
	pinnedKeys.clear()
	next.forEach((key) => pinnedKeys.add(key))
}

function onKeydown(event: KeyboardEvent) {
	if (event.key === 'Escape' && manage.value) {
		event.stopPropagation()
		cancelManage()
	}
}
function onReposition() {
	if (open.value) updatePosition()
}

// Re-read pin state whenever the set of registered pages changes (a plugin
// enabled or disabled at runtime), so a page removed while pinned drops out.
watch(() => pages.length, refreshPinned)

onMounted(() => {
	void refreshPinned()
	window.addEventListener('resize', onReposition)
	window.addEventListener('scroll', onReposition, true)
	window.addEventListener('keydown', onKeydown, true)
})
onBeforeUnmount(() => {
	cancelClose()
	window.removeEventListener('resize', onReposition)
	window.removeEventListener('scroll', onReposition, true)
	window.removeEventListener('keydown', onKeydown, true)
})
</script>

<style scoped>
.apps-nav-btn {
	width: 3rem;
	height: 3rem;
	border: none;
	background: transparent;
	border-radius: 9999px;
	display: flex;
	align-items: center;
	justify-content: center;
	cursor: pointer;
	transition: all 0.15s;
	color: var(--color-primary);
}
.apps-nav-btn:hover {
	background: var(--color-button-bg);
	color: var(--color-contrast);
}
.apps-nav-btn-active {
	color: var(--color-button-text-selected);
	background: var(--color-button-bg-selected);
}
.apps-nav-btn-open {
	color: var(--color-contrast);
	background: var(--color-button-bg);
}
.apps-icon {
	display: inline-flex;
}
.apps-icon :deep(svg) {
	width: 1.5rem;
	height: 1.5rem;
}
.plugin-apps-panel {
	position: fixed;
	z-index: 1000;
	width: 22rem;
	overflow-y: auto;
	border: 1px solid var(--color-divider);
	border-radius: 0.75rem;
	padding: 1rem;
	box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
}
.apps-header {
	display: flex;
	align-items: center;
	justify-content: space-between;
	margin-bottom: 0.75rem;
}
.apps-title {
	font-size: 1.125rem;
	font-weight: 700;
	margin: 0;
	color: var(--color-contrast);
}
.apps-manage-btn {
	display: inline-flex;
	align-items: center;
	gap: 0.25rem;
	border: none;
	background: transparent;
	cursor: pointer;
	color: var(--color-secondary);
	font-size: 0.875rem;
}
.apps-manage-btn:hover {
	color: var(--color-contrast);
}
.apps-section-label {
	color: var(--color-secondary);
	font-size: 0.8125rem;
	margin: 0.5rem 0;
}
.apps-grid {
	display: grid;
	grid-template-columns: repeat(4, 1fr);
	gap: 0.5rem;
}
.apps-tile {
	position: relative;
	display: flex;
	flex-direction: column;
	align-items: center;
	justify-content: center;
	gap: 0.375rem;
	padding: 0.625rem 0.25rem;
	border-radius: 0.5rem;
	background: transparent;
	border: none;
	color: var(--color-primary);
	text-align: center;
}
.apps-tile-click {
	cursor: pointer;
	transition: background 0.15s;
}
.apps-tile-click:hover {
	background: var(--color-button-bg);
	color: var(--color-contrast);
}
.apps-icon-lg {
	display: inline-flex;
}
.apps-icon-lg :deep(svg) {
	width: 1.75rem;
	height: 1.75rem;
}
.apps-tile-label {
	font-size: 0.75rem;
	line-height: 1.1;
	max-width: 100%;
	overflow: hidden;
	text-overflow: ellipsis;
	white-space: nowrap;
}
.apps-badge {
	position: absolute;
	top: 0.125rem;
	right: 0.125rem;
	width: 1.125rem;
	height: 1.125rem;
	border-radius: 9999px;
	border: none;
	display: flex;
	align-items: center;
	justify-content: center;
	cursor: pointer;
	color: #fff;
	padding: 0;
}
.apps-badge :deep(svg) {
	width: 0.75rem;
	height: 0.75rem;
}
.apps-badge-add {
	background: var(--color-brand);
}
.apps-badge-remove {
	background: var(--color-red, #c74b4b);
}
.apps-empty {
	color: var(--color-secondary);
	font-size: 0.8125rem;
	margin: 0.25rem 0 0.5rem;
}
.apps-footer {
	display: flex;
	justify-content: flex-end;
	gap: 0.5rem;
	margin-top: 1rem;
}
</style>


