<template>
	<Card>
		<div class="markdown-body" v-html="description" />
	</Card>
</template>

<script setup>
import { Card } from '@modrinth/ui'
import { renderHighlightedString } from '@modrinth/utils'
import { computed } from 'vue'

import { useContentTranslation } from '@/composables/use-content-translation.ts'

const props = defineProps({
	project: {
		type: Object,
		default: () => {},
	},
})

const { translateHtml } = useContentTranslation()

/*
 * Same output as `ProjectPageDescription`, with translation on top: the sanitised
 * HTML is translated rather than the markdown source, because machine translation
 * mangles link syntax, badge tables and code fences. Only text nodes are replaced,
 * so images, badges, tables and code blocks come through untouched, and no markup
 * can be introduced by the translation service.
 */
const description = computed(() =>
	translateHtml(renderHighlightedString(props.project?.body ?? '')),
)
</script>

<script>
export default {
	name: 'Description',
}
</script>
