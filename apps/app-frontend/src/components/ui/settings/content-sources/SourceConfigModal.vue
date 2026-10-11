<script setup lang="ts">
import { PlusIcon, RefreshCwIcon, TrashIcon, TriangleAlertIcon } from '@modrinth/assets'
import {
	Button,
	defineMessages,
	DropdownSelect,
	IconButton,
	Input,
	injectNotificationManager,
	NewModal,
	Toggle,
	useVIntl,
} from '@modrinth/ui'
import { computed, ref } from 'vue'

import { sourceSettingsGet, sourceSettingsSet, sourceStorageGet, sourceStorageSet } from '@/content-sources/ipc'
import { loadContentSources } from '@/content-sources/loader'
import type { ManualEntryField, SourceSettingField, SourceSummary } from '@/content-sources/types'
import { toError } from '@/helpers/errors'

import SourceEntryModal from './SourceEntryModal.vue'

const props = defineProps<{ source: SourceSummary | null }>()

const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()

/** Where the launcher keeps manually-added entries in a source's own storage. */
const MANUAL_ENTRIES_KEY = 'manualEntries'

const messages = defineMessages({
	title: { id: 'app.settings.content-sources.config.title', defaultMessage: 'Source settings' },
	noConfig: {
		id: 'app.settings.content-sources.config.none',
		defaultMessage: 'This source has nothing to configure.',
	},
	settings: {
		id: 'app.settings.content-sources.config.settings',
		defaultMessage: 'Settings',
	},
	entries: {
		id: 'app.settings.content-sources.config.entries',
		defaultMessage: 'Entries',
	},
	entriesHint: {
		id: 'app.settings.content-sources.config.entries-hint',
		defaultMessage:
			'This source has no catalogue of its own, so you add what you want to see. Each entry is one project.',
	},
	addEntry: {
		id: 'app.settings.content-sources.config.add-entry',
		defaultMessage: 'Add entry',
	},
	refreshEntries: {
		id: 'app.settings.content-sources.config.refresh-entries',
		defaultMessage: 'Refresh entries',
	},
	removeEntry: {
		id: 'app.settings.content-sources.config.remove-entry',
		defaultMessage: 'Remove',
	},
	invalid: {
		id: 'app.settings.content-sources.config.invalid',
		defaultMessage: 'Does not match the required format',
	},
	required: {
		id: 'app.settings.content-sources.config.required',
		defaultMessage: 'Required',
	},
	saved: {
		id: 'app.settings.content-sources.config.saved',
		defaultMessage: 'Changes are saved as you make them.',
	},
	done: { id: 'app.settings.content-sources.config.done', defaultMessage: 'Done' },
})

const modal = ref<InstanceType<typeof NewModal> | null>(null)
const entryModal = ref<InstanceType<typeof SourceEntryModal> | null>(null)
const values = ref<Record<string, string>>({})
/** Rows of the manual-entry form; each row is keyed by the fields' keys. */
const entries = ref<Record<string, string>[]>([])
const loading = ref(false)

const settingsFields = computed<SourceSettingField[]>(
	() => props.source?.manifest?.settings ?? [],
)
const entryFields = computed<ManualEntryField[]>(
	() => props.source?.manifest?.manual_entry ?? [],
)
const hasAnything = computed(
	() => settingsFields.value.length > 0 || entryFields.value.length > 0,
)

function defaultFor(field: SourceSettingField): string {
	if (field.type === 'toggle') return 'false'
	if (field.type === 'number') return '0'
	if (field.type === 'select') return field.options[0]?.value ?? ''
	return ''
}

function parseEntries(raw: string | null): Record<string, string>[] {
	if (!raw) return []
	try {
		const parsed = JSON.parse(raw)
		if (!Array.isArray(parsed)) return []
		return parsed
			.filter((item) => item && typeof item === 'object')
			.map((item) => {
				const row: Record<string, string> = {}
				for (const field of entryFields.value) {
					const value = (item as Record<string, unknown>)[field.key]
					row[field.key] = typeof value === 'string' ? value : ''
				}
				return row
			})
	} catch {
		return []
	}
}

/**
 * Fetches and parses the entry list. `persistEntries` writes this list back
 * wholesale, so it only ever writes after a successful read: wiping the
 * source's entries because one read failed would be the one mistake this modal
 * can never take back.
 */
const entriesLoaded = ref(false)
const reloadingEntries = ref(false)
/**
 * Set once the user has changed the list in this session. A reload must never
 * overwrite those changes with what is on disk: the post-open refresh below
 * fires on a timer, and if it lands between a row being added and its write
 * completing it would read the old list and drop the new row from the UI.
 */
let entriesDirty = false

async function reloadEntries(): Promise<void> {
	const source = props.source
	if (!source) return
	reloadingEntries.value = true
	try {
		const raw = await sourceStorageGet(source.id, MANUAL_ENTRIES_KEY)
		console.debug('[content-source-config]', source.id, 'manualEntries raw:', raw)
		const parsed = parseEntries(raw)
		console.debug('[content-source-config]', source.id, 'parsed entries:', parsed)
		if (entriesDirty) {
			// The user's list is authoritative now; keep it and just record that a
			// read succeeded, so a later write is still allowed.
			entriesLoaded.value = true
			return
		}
		entries.value = parsed
		entriesLoaded.value = true
	} catch (error) {
		console.error('[content-source-config]', source.id, 'loading entries failed:', error)
		throw error
	} finally {
		reloadingEntries.value = false
	}
}

async function load() {
	const source = props.source
	if (!source) return
	loading.value = true
	try {
		const saved = await sourceSettingsGet(source.id)
		const next: Record<string, string> = {}
		for (const field of settingsFields.value) {
			next[field.key] = saved[field.key] ?? field.default ?? defaultFor(field)
		}
		values.value = next

		await reloadEntries()
	} catch (error) {
		handleError(toError(error))
	} finally {
		loading.value = false
	}
}

/** Coalesces bursts of typing into one write. */
function debounce(work: () => void, delay = 400): () => void {
	let timer: ReturnType<typeof setTimeout> | undefined
	return () => {
		if (timer !== undefined) clearTimeout(timer)
		timer = setTimeout(work, delay)
	}
}

const scheduleSettingsSave = debounce(() => void saveAllSettings())

const pendingSettings = ref<Record<string, string>>({})

function setSetting(field: SourceSettingField, value: string) {
	values.value = { ...values.value, [field.key]: value }
	pendingSettings.value = { ...pendingSettings.value, [field.key]: value }
	scheduleSettingsSave()
}

/**
 * Write only what changed. A text field saves as the user types (debounced), so
 * the whole form is not rewritten on every keystroke of every field.
 */
async function saveAllSettings() {
	const source = props.source
	if (!source) return
	const changed = Object.entries(pendingSettings.value)
	if (changed.length === 0) return
	pendingSettings.value = {}
	try {
		for (const [key, value] of changed) {
			await sourceSettingsSet(source.id, key, value)
		}
		// A setting usually changes what the source does, so reload it the same
		// way a plugin is reloaded after its settings change.
		await loadContentSources()
	} catch (error) {
		handleError(toError(error))
	}
}

/**
 * Persist the whole entry list. Entries are small and the source reads them as
 * one JSON array, so writing the array wholesale is simpler and less error-prone
 * than tracking a diff — and it is what the source expects to read back.
 */
async function persistEntries() {
	const source = props.source
	if (!source) return
	if (!entriesLoaded.value) {
		// Nothing was read this session; writing now would save the empty list
		// over entries that exist on disk.
		console.warn('[content-source-config]', source.id, 'skipping entry save: nothing was loaded')
		return
	}
	// Drop rows the user left entirely blank, so an accidental "Add" does not
	// leave a nameless entry behind for the source to trip over.
	const rows = entries.value.filter((row) =>
		entryFields.value.some((field) => (row[field.key] ?? '').trim().length > 0),
	)
	try {
		await sourceStorageSet(source.id, MANUAL_ENTRIES_KEY, JSON.stringify(rows))
	} catch (error) {
		handleError(toError(error))
	}
}

/**
 * Adding and editing happen in the entry modal, so the list stays a list: one
 * row per entry, and nothing expands in place. On save the row replaces its
 * previous self (or joins the end) and the whole list is persisted, the same
 * write the source reads back as one JSON array.
 */
function handleEntrySave(row: Record<string, string>, editIndex: number | undefined) {
	entriesDirty = true
	if (editIndex === undefined) {
		entries.value = [...entries.value, row]
	} else {
		const next = [...entries.value]
		next[editIndex] = row
		entries.value = next
	}
	void persistEntries()
}

function openAddEntry() {
	entryModal.value?.show(undefined)
}

function openEditEntry(index: number) {
	entryModal.value?.show(entries.value[index], index)
}

function removeEntry(index: number) {
	entriesDirty = true
	entries.value = entries.value.filter((_, position) => position !== index)
	void persistEntries()
}

function entryIssue(row: Record<string, string>, field: ManualEntryField): string | null {
	const value = (row[field.key] ?? '').trim()
	if (field.required && !value) return formatMessage(messages.required)
	if (value && field.pattern) {
		try {
			if (!new RegExp(field.pattern).test(value)) {
				return formatMessage(messages.invalid)
			}
		} catch {
			// A broken pattern in a manifest must not block the user's input; the
			// source's own validation is the real gate.
		}
	}
	return null
}

/** One row's read-only summary. The name — or the repo, when unnamed — is the
 * title; those two keys lead so a type-only row never becomes the title. */
function entrySummary(row: Record<string, string>): { title: string; detail: string } {
	const rank = (key: string) => (key === 'name' ? 0 : key === 'repo' ? 1 : 2)
	const parts = entryFields.value
		.map((field) => ({ key: field.key, label: field.label, value: (row[field.key] ?? '').trim() }))
		.filter((part) => part.value.length > 0)
		.sort((a, b) => rank(a.key) - rank(b.key))
	if (parts.length === 0) return { title: '—', detail: '' }
	const [first, ...rest] = parts
	return {
		title: first.value,
		detail: rest.map((part) => `${part.label}：${part.value}`).join(' · '),
	}
}

/** Timers of the post-open refreshes, cleared when the modal leaves. */
let refreshTimers: ReturnType<typeof setTimeout>[] = []

function clearRefreshTimers() {
	for (const timer of refreshTimers) clearTimeout(timer)
	refreshTimers = []
}

async function show() {
	// A fresh open reads the source's current entries again, so the previous
	// session's edits are no longer authoritative.
	entriesDirty = false
	// Open on whatever was read last; the reload below brings the list current.
	const opening = load()
	modal.value?.show()
	await opening

	// The storage read at open time has, in practice, come back empty more than
	// once for reasons the read itself does not explain. Rather than guess at
	// the race, re-read twice more after the modal has settled — each pass logs
	// what it got, and a pass that finds nothing changes nothing that matters
	// (an empty answer cannot erase rows the user is editing, because the guard
	// in persistEntries only writes after a successful read).
	clearRefreshTimers()
	refreshTimers = [1_000, 2_000].map((delay) =>
		setTimeout(() => {
			if (!modal.value) return
			void reloadEntries().catch((error) => handleError(toError(error)))
		}, delay),
	)
}

/**
 * The X button, Escape and clicking the overlay all close through NewModal
 * itself rather than through `hide` below, so a debounced edit would be lost
 * when the modal leaves. This hook is the one path every close takes.
 */
function flush() {
	clearRefreshTimers()
	void saveAllSettings()
	void persistEntries()
}

async function hide() {
	// Flush anything the debounce has not written yet, so closing the modal
	// never loses the last edit.
	await Promise.all([saveAllSettings(), persistEntries()])
	modal.value?.hide()
}

defineExpose({ show, hide })
</script>

<template>
	<NewModal
		ref="modal"
		:header="formatMessage(messages.title)"
		max-width="min(640px, 92vw)"
		:on-hide="flush"
	>
		<div class="flex w-full max-w-[600px] flex-col gap-4">
			<div v-if="source" class="flex flex-col">
				<h3 class="m-0 text-lg font-semibold text-contrast">
					{{ source.manifest?.name ?? source.dir_name }}
				</h3>
				<span v-if="source.manifest?.version" class="text-sm text-secondary">
					v{{ source.manifest.version }}
				</span>
			</div>

			<p v-if="loading" class="m-0 text-secondary">…</p>
			<p v-else-if="!hasAnything" class="m-0 text-secondary">
				{{ formatMessage(messages.noConfig) }}
			</p>

			<template v-else>
				<div v-if="entryFields.length" class="flex flex-col gap-3">
					<div class="flex items-center justify-between gap-2">
						<div class="flex flex-col gap-1">
							<h4 class="m-0 text-sm font-semibold text-contrast">
								{{ formatMessage(messages.entries) }}
							</h4>
							<span class="text-xs text-secondary">{{ formatMessage(messages.entriesHint) }}</span>
						</div>
						<IconButton
							v-tooltip="formatMessage(messages.refreshEntries)"
							:label="formatMessage(messages.refreshEntries)"
							:disabled="reloadingEntries"
							@click="reloadEntries().catch((error) => handleError(toError(error)))"
						>
							<RefreshCwIcon :class="reloadingEntries ? 'animate-spin' : ''" />
						</IconButton>
					</div>

					<div
						v-for="(row, index) in entries"
						:key="index"
						class="flex cursor-pointer items-center justify-between gap-3 rounded-xl border border-solid border-surface-5 bg-surface-3 px-3 py-2"
						role="button"
						tabindex="0"
						@click="openEditEntry(index)"
						@keydown.enter="openEditEntry(index)"
					>
						<div class="flex min-w-0 flex-col gap-0.5">
							<span class="min-w-0 break-words text-sm font-medium text-contrast">
								{{ entrySummary(row).title }}
							</span>
							<span v-if="entrySummary(row).detail" class="min-w-0 break-words text-xs text-secondary">
								{{ entrySummary(row).detail }}
							</span>
							<span
								v-if="entryFields.some((field) => entryIssue(row, field))"
								class="flex items-center gap-1 text-xs text-orange"
							>
								<TriangleAlertIcon class="size-3.5" />
								{{ entryFields.filter((field) => entryIssue(row, field)).map((field) => entryIssue(row, field)).join(' · ') }}
							</span>
						</div>
						<Button
							size="sm"
							type="outlined"
							@click.stop="removeEntry(index)"
						>
							<TrashIcon />
							{{ formatMessage(messages.removeEntry) }}
						</Button>
					</div>

					<Button class="w-full" type="outlined" @click="openAddEntry">
						<PlusIcon />
						{{ formatMessage(messages.addEntry) }}
					</Button>
				</div>

				<div v-if="settingsFields.length" class="flex flex-col gap-4 border-0 border-t border-solid border-surface-5 pt-4">
					<h4 class="m-0 text-sm font-semibold text-contrast">
						{{ formatMessage(messages.settings) }}
					</h4>
					<div v-for="field in settingsFields" :key="field.key" class="flex flex-col gap-1">
						<div class="flex items-center justify-between gap-4">
							<label class="text-sm font-medium text-contrast">{{ field.label }}</label>
							<Toggle
								v-if="field.type === 'toggle'"
								:model-value="values[field.key] === 'true'"
								@update:model-value="(value: boolean) => setSetting(field, String(value))"
							/>
							<DropdownSelect
								v-else-if="field.type === 'select'"
								:model-value="values[field.key]"
								:options="field.options.map((option) => option.value)"
								:display-name="
									(value: string) =>
										field.options.find((option) => option.value === value)?.label ?? value
								"
								name="Source setting"
								@update:model-value="(value: string) => setSetting(field, value)"
							/>
						</div>
						<Input
							v-if="field.type === 'text' || field.type === 'number'"
							:model-value="values[field.key]"
							:type="field.type === 'number' ? 'number' : 'text'"
							wrapper-class="w-full"
							@update:model-value="(value: string) => setSetting(field, value)"
						/>
						<span v-if="field.description" class="text-xs text-secondary">
							{{ field.description }}
						</span>
					</div>
				</div>
			</template>

			<div class="flex items-center justify-between gap-2 pt-2">
				<span class="text-xs text-secondary">{{ formatMessage(messages.saved) }}</span>
				<Button @click="hide">{{ formatMessage(messages.done) }}</Button>
			</div>

			<!-- The entry editor lives inside this modal's slot so the component keeps
				one root. Both modals teleport to body, so nothing is DOM-nested here —
				this is the same arrangement as every other host of a secondary modal. -->
			<SourceEntryModal ref="entryModal" :source="source" @save="handleEntrySave" />
		</div>
	</NewModal>
</template>
