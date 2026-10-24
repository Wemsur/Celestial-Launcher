<script setup lang="ts">
/**
 * Which content sources Discover pulls from, as a row of the sources' own marks.
 *
 * A mark is the whole control: clicking it turns that source on or off, so the
 * state of every source is readable at a glance instead of hiding behind a
 * dropdown. Off is drawn greyed out rather than removed, because a source that
 * vanished from the row would look uninstalled rather than merely excluded.
 *
 * Double-clicking a mark turns every other source off, the usual "just show me
 * this one" gesture. The toggle is applied on the click itself rather than held
 * back for the double-click window — waiting would make every ordinary click
 * feel laggy. A double-click then costs two toggles that cancel out, followed by
 * the solo, which does not depend on where those toggles left the source. The
 * search behind it is debounced, so the whole gesture runs one query.
 */
import { XIcon } from '@modrinth/assets'
import { SourceBadge, defineMessages, useVIntl } from '@modrinth/ui'
import { computed } from 'vue'

import { selectableSources, sourceAppearance } from '@/content-sources/catalog'
import { isSourceActive, setSourcesActive } from '@/content-sources/preferences'

const { formatMessage } = useVIntl()

const messages = defineMessages({
	clearAll: {
		id: 'app.browse.content-sources.clear-all',
		defaultMessage: 'Clear all',
	},
	turnOn: {
		id: 'app.browse.content-sources.turn-on',
		defaultMessage: 'Show content from {source}',
	},
	turnOff: {
		id: 'app.browse.content-sources.turn-off',
		defaultMessage: 'Hide content from {source}',
	},
})

const sources = computed(() =>
	selectableSources().map((source) => ({
		id: source.id,
		...sourceAppearance(source.id),
	})),
)

/** Whether a source currently contributes to the merged list. */
function active(sourceId: string): boolean {
	return isSourceActive(sourceId)
}

function toggle(sourceId: string): void {
	setSourcesActive([{ id: sourceId, active: !active(sourceId) }])
}

function solo(sourceId: string): void {
	setSourcesActive(
		sources.value.map((source) => ({
			id: source.id,
			active: source.id === sourceId,
		})),
	)
}

function clearAll(): void {
	setSourcesActive(sources.value.map((source) => ({ id: source.id, active: false })))
}

/** A mark's tooltip: what clicking it does, in the source's own name. */
function hint(source: { id: string; name: string }): string {
	return formatMessage(active(source.id) ? messages.turnOff : messages.turnOn, {
		source: source.name,
	})
}
</script>

<template>
	<div class="flex shrink-0 items-center gap-1.5">
		<button
			v-for="source in sources"
			:key="source.id"
			v-tooltip="hint(source)"
			type="button"
			class="flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-full border-0 bg-surface-4 p-0 transition duration-100 ease-out active:scale-90"
			:class="active(source.id) ? '' : 'opacity-40 grayscale'"
			:aria-pressed="active(source.id)"
			:aria-label="source.name"
			@click="toggle(source.id)"
			@dblclick="solo(source.id)"
		>
			<SourceBadge
				:label="source.name"
				:color="source.color"
				:icon-url="source.iconUrl"
				:is-modrinth="source.isModrinth"
				size="20px"
			/>
		</button>

		<button
			v-tooltip="formatMessage(messages.clearAll)"
			type="button"
			class="flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-full border-0 bg-surface-4 p-0 text-secondary transition duration-100 ease-out hover:text-contrast active:scale-90"
			:aria-label="formatMessage(messages.clearAll)"
			@click="clearAll"
		>
			<XIcon class="size-4" />
		</button>
	</div>
</template>
