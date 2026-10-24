<script setup lang="ts">
/**
 * Content from sources that could not join the sorted list.
 *
 * A source with no number behind the requested sort cannot be ordered against
 * the others, so its rows would sort to the bottom and be cut by the page limit —
 * it would simply not appear. Rather than drop it, its content is shown here,
 * below the list and its pager, in its own source's order.
 *
 * Each source keeps its own page counter: this block is a side channel, not part
 * of the main list's paging, so turning to the next batch here does not move the
 * list above and vice versa.
 */
import { RefreshCwIcon, SpinnerIcon } from '@modrinth/assets'
import {
	Button,
	ProjectCard,
	ProjectCardList,
	defineMessages,
	useVIntl,
} from '@modrinth/ui'
import { computed, reactive, watch } from 'vue'

import { sourceAppearance } from '@/content-sources/catalog'
import type { RecommendationCard, RecommendationSource } from '@/content-sources/recommendations'

const props = withDefaults(
	defineProps<{
		sources: RecommendationSource[]
		/** How many cards a batch holds. */
		limit?: number
		/**
		 * The layout the list above is using, so these cards look like the ones
		 * they sit under. A source's cards usually carry no banner image, and the
		 * grid layout reserves a banner strip — which would then be filled with
		 * the placeholder Modrinth artwork.
		 */
		layout?: 'list' | 'grid'
		/** The next batch from one source. Pages are 1-based. */
		load: (sourceId: string, page: number) => Promise<RecommendationCard[]>
	}>(),
	{ limit: 6, layout: 'list' },
)

const { formatMessage } = useVIntl()

const messages = defineMessages({
	browseTitle: {
		id: 'app.browse.recommendations.title',
		defaultMessage: 'Browse · {source}',
	},
	next: {
		id: 'app.browse.recommendations.next',
		defaultMessage: 'Show different ones',
	},
})

interface GroupState {
	items: RecommendationCard[]
	page: number
	loading: boolean
}

const states = reactive<Record<string, GroupState>>({})

// A new search replaces the first batches; the counter restarts with it, since
// page 2 of a different query means nothing here.
watch(
	() => props.sources,
	(sources) => {
		for (const key of Object.keys(states)) delete states[key]
		for (const source of sources) {
			states[source.sourceId] = { items: source.items, page: 1, loading: false }
		}
	},
	{ immediate: true },
)

/** How many batches a source has, so "different ones" can wrap around. */
function batchCount(total: number): number {
	return Math.max(1, Math.ceil(total / props.limit))
}

async function nextBatch(source: RecommendationSource): Promise<void> {
	const state = states[source.sourceId]
	if (!state || state.loading) return

	const pages = batchCount(source.total)
	const page = (state.page % pages) + 1
	state.loading = true
	try {
		const items = await props.load(source.sourceId, page)
		// An empty batch means the source's reported total overestimated what it
		// can serve; keep what is on screen rather than blanking the block.
		if (items.length > 0) {
			state.items = items
			state.page = page
		}
	} catch (error) {
		// A failed batch leaves the current one up: the block is supplementary, so
		// losing it silently is better than replacing it with an error.
		console.error(`[content-sources] "${source.sourceId}" recommendation batch failed`, error)
	} finally {
		state.loading = false
	}
}

interface VisibleGroup {
	sourceId: string
	total: number
	appearance: ReturnType<typeof sourceAppearance>
	state: GroupState
}

const visible = computed<VisibleGroup[]>(() => {
	const groups: VisibleGroup[] = []
	for (const source of props.sources) {
		const state = states[source.sourceId]
		if (!state || state.items.length === 0) continue
		groups.push({
			sourceId: source.sourceId,
			total: source.total,
			appearance: sourceAppearance(source.sourceId),
			state,
		})
	}
	return groups
})
</script>

<template>
	<div v-if="visible.length > 0" class="flex flex-col gap-6">
		<section
			v-for="source in visible"
			:key="source.sourceId"
			class="flex flex-col gap-3 border-0 border-t-[1px] border-solid border-surface-5 pt-4"
		>
			<div class="flex items-center gap-2">
				<h2 class="m-0 text-base font-semibold text-primary">
					{{ formatMessage(messages.browseTitle, { source: source.appearance.name }) }}
				</h2>
				<Button
					class="ml-auto"
					type="quiet"
					size="sm"
					:disabled="source.state.loading || batchCount(source.total) < 2"
					@click="nextBatch(source)"
				>
					<SpinnerIcon v-if="source.state.loading" class="animate-spin" />
					<RefreshCwIcon v-else />
					{{ formatMessage(messages.next) }}
				</Button>
			</div>

			<ProjectCardList :layout="layout">
				<ProjectCard
					v-for="card in source.state.items"
					:key="card.id"
					:layout="layout"
					:link="card.link"
					:title="card.name"
					:icon-url="card.iconUrl"
					:summary="card.summary"
					:tags="card.tags"
					:source-color="source.appearance.color"
					:source-tag="{
						label: source.appearance.name,
						color: source.appearance.color,
						iconUrl: source.appearance.iconUrl,
						isModrinth: source.appearance.isModrinth,
					}"
					:max-tags="2"
				/>
			</ProjectCardList>
		</section>
	</div>
</template>
