<template>
	<div v-if="images.length" class="grid grid-cols-2 gap-3 md:grid-cols-3">
		<a
			v-for="image in images"
			:key="image.url"
			:href="image.url"
			class="group relative overflow-hidden rounded-2xl border border-solid border-surface-4 bg-surface-3"
			@click.prevent="open(image.url)"
		>
			<img
				:src="image.url"
				:alt="image.title ?? ''"
				loading="lazy"
				class="h-full w-full object-cover transition-transform group-hover:scale-[1.02]"
			/>
		</a>
	</div>
	<Card v-else>
		<p class="m-0 text-secondary">{{ formatMessage(messages.empty) }}</p>
	</Card>
</template>

<script setup lang="ts">
import { Card, defineMessages, useVIntl } from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed } from 'vue'

import { useSourceProject } from '@/composables/use-source-project'

const { formatMessage } = useVIntl()
const { project } = useSourceProject()

const messages = defineMessages({
	empty: {
		id: 'app.source-project.no-gallery',
		defaultMessage: 'This source does not provide a gallery.',
	},
})

const images = computed(() => project.value?.gallery ?? [])

/**
 * No lightbox: the images are shown at their own size in a new window rather
 * than in a launcher-built viewer, so a large screenshot is not downscaled into
 * the pane it was opened from.
 */
function open(url: string) {
	void openUrl(url)
}
</script>
