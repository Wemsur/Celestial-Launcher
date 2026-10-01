<script setup lang="ts">
import { PlusIcon, TrashIcon, TriangleAlertIcon } from '@modrinth/assets'
import {
	Button,
	defineMessages,
	DropdownSelect,
	Input,
	injectNotificationManager,
	NewModal,
	Toggle,
	useVIntl,
} from '@modrinth/ui'
import { computed, ref } from 'vue'

import { sourceSettingsSet, sourceStorageGet, sourceStorageSet } from '@/content-sources/ipc'
import { loadContentSources } from '@/content-sources/loader'
import type { ManualEntryField, SourceSettingField, SourceSummary } from '@/content-sources/types'

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

		entries.value = parseEntries(await sourceStorageGet(source.id, MANUAL_ENTRIES_KEY))
	} catch (error) {
		handleError(error instanceof Error ? error : new Error(String(error)))
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
const scheduleEntriesSave = debounce(() => void persistEntries())

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
		handleError(error instanceof Error ? error : new Error(String(error)))
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
	// Drop rows the user left entirely blank, so an accidental "Add" does not
	// leave a nameless entry behind for the source to trip over.
	const rows = entries.value.filter((row) =>
		entryFields.value.some((field) => (row[field.key] ?? '').trim().length > 0),
	)
	try {
		await sourceStorageSet(source.id, MANUAL_ENTRIES_KEY, JSON.stringify(rows))
	} catch (error) {
		handleError(error instanceof Error ? error : new Error(String(error)))
	}
}

function addEntry() {
	const row: Record<string, string> = {}
	for (const field of entryFields.value) {
		// A choice field starts on its first option so a new row is valid from the
		// start, rather than empty until the user touches the dropdown.
		row[field.key] = field.options[0]?.value ?? ''
	}
	entries.value = [...entries.value, row]
}

function removeEntry(index: number) {
	entries.value = entries.value.filter((_, position) => position !== index)
	void persistEntries()
}

function setEntryField(index: number, key: string, value: string) {
	const next = [...entries.value]
	next[index] = { ...next[index], [key]: value }
	entries.value = next
	scheduleEntriesSave()
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

async function show() {
	await load()
	modal.value?.show()
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
	<NewModal ref="modal" :header="formatMessage(messages.title)" max-width="min(640px, 92vw)">
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
				<div v-if="settingsFields.length" class="flex flex-col gap-4">
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

				<div v-if="entryFields.length" class="flex flex-col gap-3 border-0 border-t border-solid border-surface-5 pt-4">
					<div class="flex flex-col gap-1">
						<h4 class="m-0 text-sm font-semibold text-contrast">
							{{ formatMessage(messages.entries) }}
						</h4>
						<span class="text-xs text-secondary">{{ formatMessage(messages.entriesHint) }}</span>
					</div>

					<div v-for="(row, index) in entries" :key="index" class="flex flex-col gap-2 rounded-xl border border-solid border-surface-5 bg-surface-3 p-3">
						<div v-for="field in entryFields" :key="field.key" class="flex flex-col gap-1">
							<label class="text-xs font-medium text-secondary">{{ field.label }}</label>
							<DropdownSelect
								v-if="field.options.length"
								:model-value="row[field.key]"
								:options="field.options.map((option) => option.value)"
								:display-name="
									(value: string) =>
										field.options.find((option) => option.value === value)?.label ?? value
								"
								name="Entry field"
								@update:model-value="(value: string) => setEntryField(index, field.key, value)"
							/>
							<Input
								v-else
								:model-value="row[field.key]"
								type="text"
								wrapper-class="w-full"
								:placeholder="field.placeholder ?? ''"
								@update:model-value="(value: string) => setEntryField(index, field.key, value)"
							/>
							<span v-if="field.description" class="text-xs text-secondary">
								{{ field.description }}
							</span>
							<span
								v-if="entryIssue(row, field)"
								class="flex items-center gap-1 text-xs text-orange"
							>
								<TriangleAlertIcon class="size-3.5" />
								{{ entryIssue(row, field) }}
							</span>
						</div>
						<div class="flex justify-end">
							<Button size="sm" type="outlined" @click="removeEntry(index)">
								<TrashIcon />
								{{ formatMessage(messages.removeEntry) }}
							</Button>
						</div>
					</div>

					<div>
						<Button type="outlined" @click="addEntry">
							<PlusIcon />
							{{ formatMessage(messages.addEntry) }}
						</Button>
					</div>
				</div>
			</template>

			<div class="flex items-center justify-between gap-2 pt-2">
				<span class="text-xs text-secondary">{{ formatMessage(messages.saved) }}</span>
				<Button @click="hide">{{ formatMessage(messages.done) }}</Button>
			</div>
		</div>
	</NewModal>
</template>
