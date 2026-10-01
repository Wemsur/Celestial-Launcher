<script setup lang="ts">
import { ArrowDownIcon, ArrowUpIcon, RotateCounterClockwiseIcon } from '@modrinth/assets'
import { Button, defineMessages, Input, NewModal, useVIntl } from '@modrinth/ui'
import { ref, watch } from 'vue'

import {
	DEFAULT_MULTIPLIER,
	MAX_MULTIPLIER,
	MIN_MULTIPLIER,
	normalizeMultiplier,
	resetSourceMultipliers,
	setSourceMultiplier,
	sourceMultiplier,
} from '@/content-sources/preferences'

const { formatMessage } = useVIntl()

const props = defineProps<{
	sources: { id: string; name: string }[]
}>()

const modal = ref<InstanceType<typeof NewModal> | null>(null)

// The text in each box, so a half-typed value (an empty box, a trailing dot) is
// not clobbered mid-keystroke. The stored value is committed on every change.
const localValues = ref<Record<string, string>>({})

function seed() {
	const next: Record<string, string> = {}
	for (const source of props.sources) {
		next[source.id] = String(sourceMultiplier(source.id))
	}
	localValues.value = next
}

watch(() => props.sources, seed, { immediate: true })

function setLocal(sourceId: string, value: string) {
	localValues.value = { ...localValues.value, [sourceId]: value }
}

const messages = defineMessages({
	title: {
		id: 'app.settings.content-sources.multiplier.title',
		defaultMessage: 'Source multipliers',
	},
	description: {
		id: 'app.settings.content-sources.multiplier.description',
		defaultMessage:
			'A source\'s download counts are multiplied by its factor before the merged list is sorted by downloads. Use it to stop a large site from crowding out a small one.',
	},
	source: {
		id: 'app.settings.content-sources.multiplier.source',
		defaultMessage: 'Source',
	},
	multiplier: {
		id: 'app.settings.content-sources.multiplier.label',
		defaultMessage: 'Multiplier',
	},
	reset: {
		id: 'app.settings.content-sources.multiplier.reset',
		defaultMessage: 'Reset all to 1',
	},
	resetOne: {
		id: 'app.settings.content-sources.multiplier.reset-one',
		defaultMessage: 'Reset',
	},
	increase: {
		id: 'app.settings.content-sources.multiplier.increase',
		defaultMessage: 'Double',
	},
	decrease: {
		id: 'app.settings.content-sources.multiplier.decrease',
		defaultMessage: 'Halve',
	},
	done: {
		id: 'app.settings.content-sources.multiplier.done',
		defaultMessage: 'Done',
	},
})

function valueOf(sourceId: string): string {
	return localValues.value[sourceId] ?? String(sourceMultiplier(sourceId))
}

function commit(sourceId: string, raw: string | number | undefined) {
	const text = raw === undefined || raw === null ? '' : String(raw)
	setLocal(sourceId, text)
	const parsed = Number.parseFloat(text)
	// An empty or unparseable box falls back to the default rather than 0, which
	// would silently sink every one of that source's items.
	setSourceMultiplier(sourceId, Number.isFinite(parsed) ? parsed : DEFAULT_MULTIPLIER)
}

function scale(sourceId: string, factor: number) {
	const next = normalizeMultiplier(sourceMultiplier(sourceId) * factor)
	setSourceMultiplier(sourceId, next)
	setLocal(sourceId, String(next))
}

function resetOne(sourceId: string) {
	setSourceMultiplier(sourceId, DEFAULT_MULTIPLIER)
	setLocal(sourceId, String(DEFAULT_MULTIPLIER))
}

function resetAll() {
	resetSourceMultipliers()
	seed()
}

function show() {
	seed()
	modal.value?.show()
}

function hide() {
	modal.value?.hide()
}

defineExpose({ show, hide })
</script>

<template>
	<NewModal ref="modal" :header="formatMessage(messages.title)">
		<div class="flex flex-col gap-4 w-[520px] max-w-full">
			<p class="m-0 text-sm text-secondary">
				{{ formatMessage(messages.description) }}
			</p>

			<div class="flex items-center gap-3 text-xs font-semibold text-secondary uppercase">
				<span class="flex-1">{{ formatMessage(messages.source) }}</span>
				<span>{{ formatMessage(messages.multiplier) }}</span>
			</div>

			<div class="flex flex-col gap-2">
				<div
					v-for="source in props.sources"
					:key="source.id"
					class="flex items-center gap-2"
				>
					<span class="flex-1 min-w-0 truncate text-sm text-contrast">{{ source.name }}</span>

					<div class="flex items-center gap-1">
						<Input
							:model-value="valueOf(source.id)"
							type="number"
							size="small"
							wrapper-class="w-32"
							@update:model-value="(value: string | number | undefined) => commit(source.id, value)"
						/>
						<Button
							size="sm"
							type="outlined"
							:disabled="sourceMultiplier(source.id) >= MAX_MULTIPLIER"
							:aria-label="formatMessage(messages.increase)"
							@click="scale(source.id, 2)"
						>
							<ArrowUpIcon />
						</Button>
						<Button
							size="sm"
							type="outlined"
							:disabled="sourceMultiplier(source.id) <= MIN_MULTIPLIER"
							:aria-label="formatMessage(messages.decrease)"
							@click="scale(source.id, 0.5)"
						>
							<ArrowDownIcon />
						</Button>
						<Button
							size="sm"
							type="outlined"
							:disabled="sourceMultiplier(source.id) === DEFAULT_MULTIPLIER"
							:aria-label="formatMessage(messages.resetOne)"
							@click="resetOne(source.id)"
						>
							<RotateCounterClockwiseIcon />
						</Button>
					</div>
				</div>
			</div>

			<div class="flex items-center justify-between gap-2 pt-2">
				<Button type="outlined" @click="resetAll">
					{{ formatMessage(messages.reset) }}
				</Button>
				<Button @click="hide">{{ formatMessage(messages.done) }}</Button>
			</div>
		</div>
	</NewModal>
</template>
