<script setup lang="ts">
import { Button, defineMessages, DropdownSelect, Input, NewModal, useVIntl } from '@modrinth/ui'
import { computed, ref } from 'vue'

import type { ManualEntryField, SourceSummary } from '@/content-sources/types'

const props = defineProps<{ source: SourceSummary | null }>()

const { formatMessage } = useVIntl()

const emit = defineEmits<{
	save: [row: Record<string, string>, editIndex: number | undefined]
}>()

const messages = defineMessages({
	title: { id: 'app.settings.content-sources.entry.title', defaultMessage: 'Add entry' },
	editTitle: { id: 'app.settings.content-sources.entry.edit', defaultMessage: 'Edit entry' },
	cancel: { id: 'app.settings.content-sources.entry.cancel', defaultMessage: 'Cancel' },
	confirm: { id: 'app.settings.content-sources.entry.confirm', defaultMessage: 'Save' },
	invalid: {
		id: 'app.settings.content-sources.entry.invalid',
		defaultMessage: 'Does not match the required format',
	},
	required: {
		id: 'app.settings.content-sources.entry.required',
		defaultMessage: 'Required',
	},
})

const modal = ref<InstanceType<typeof NewModal> | null>(null)
const draft = ref<Record<string, string>>({})
const editIndex = ref<number | undefined>(undefined)

const fields = computed<ManualEntryField[]>(() => props.source?.manifest?.manual_entry ?? [])

const editing = computed(() => editIndex.value !== undefined)

/**
 * Opens the editor. `prefill` is the row being edited, or nothing for a new
 * entry; a choice field starts on its first option so a new entry is valid
 * before the user touches its dropdown.
 */
function show(prefill?: Record<string, string>, index?: number) {
	editIndex.value = index
	const next: Record<string, string> = {}
	for (const field of fields.value) {
		const value = prefill?.[field.key]
		next[field.key] = value ?? field.options[0]?.value ?? ''
	}
	draft.value = next
	modal.value?.show()
}

function setField(key: string, value: string) {
	draft.value = { ...draft.value, [key]: value }
}

function issue(field: ManualEntryField): string | null {
	const value = (draft.value[field.key] ?? '').trim()
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

const valid = computed(() => fields.value.every((field) => issue(field) === null))

function confirm() {
	if (!valid.value) return
	emit('save', { ...draft.value }, editIndex.value)
	modal.value?.hide()
}

function cancel() {
	modal.value?.hide()
}

defineExpose({ show })
</script>

<template>
	<NewModal
		ref="modal"
		:header="formatMessage(editing ? messages.editTitle : messages.title)"
		max-width="min(560px, 92vw)"
	>
		<div class="flex w-full max-w-[520px] flex-col gap-4">
			<div v-for="field in fields" :key="field.key" class="flex flex-col gap-1">
				<label class="text-sm font-medium text-contrast">{{ field.label }}</label>
				<DropdownSelect
					v-if="field.options.length"
					:model-value="draft[field.key]"
					:options="field.options.map((option) => option.value)"
					:display-name="
						(value: string) =>
							field.options.find((option) => option.value === value)?.label ?? value
					"
					name="Entry field"
					@update:model-value="(value: string) => setField(field.key, value)"
				/>
				<Input
					v-else
					:model-value="draft[field.key]"
					type="text"
					wrapper-class="w-full"
					:placeholder="field.placeholder ?? ''"
					@update:model-value="(value: string) => setField(field.key, value)"
				/>
				<span v-if="field.description" class="text-xs text-secondary">
					{{ field.description }}
				</span>
				<span v-if="issue(field)" class="flex items-center gap-1 text-xs text-orange">
					{{ issue(field) }}
				</span>
			</div>

			<div class="flex items-center justify-end gap-2 pt-2">
				<Button type="outlined" @click="cancel">
					{{ formatMessage(messages.cancel) }}
				</Button>
				<Button :disabled="!valid" @click="confirm">
					{{ formatMessage(messages.confirm) }}
				</Button>
			</div>
		</div>
	</NewModal>
</template>
