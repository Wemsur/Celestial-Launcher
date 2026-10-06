<template>
	<Card>
		<div
			v-if="body && isMarkdown"
			class="markdown-body"
			v-html="rendered"
		/>
		<p v-else-if="body" class="m-0 whitespace-pre-wrap text-primary">{{ body }}</p>
		<p v-else class="m-0 text-secondary">
			{{ formatMessage(messages.empty) }}
		</p>
	</Card>
</template>

<script setup lang="ts">
import { Card, defineMessages, useVIntl } from '@modrinth/ui'
import { renderHighlightedString } from '@modrinth/utils'
import { computed } from 'vue'

import { installedManifest } from '@/content-sources/catalog'
import { useSourceProject } from '@/composables/use-source-project'

const { formatMessage } = useVIntl()
const { sourceId, project } = useSourceProject()

const messages = defineMessages({
	empty: {
		id: 'app.source-project.no-description',
		defaultMessage: 'This source does not provide a description.',
	},
})

const body = computed(() => project.value?.body ?? '')

/**
 * Whether the body is Markdown is the source's call, declared in its manifest:
 * a README and a site's `content_md` are Markdown, but a source that publishes
 * plain text should not have its asterisks and hashes interpreted.
 *
 * Read from the installed-source list, which is where the backend puts a
 * manifest. The provider a source registers at runtime carries only its own
 * functions, so asking it for the manifest finds nothing.
 */
const isMarkdown = computed(
	() => (installedManifest(sourceId.value)?.detail_format ?? 'markdown') === 'markdown',
)

// Sanitised markdown, the same renderer the Modrinth project description uses,
// so both kinds of project page format their body identically. The source's own
// hosts are passed so its body's images render directly — the shared filter
// would otherwise proxy them to wsrv.nl, which a source's CDN rarely survives.
const sourceHosts = computed(() => installedManifest(sourceId.value)?.hosts ?? [])
const rendered = computed(() => renderHighlightedString(body.value, sourceHosts.value))
</script>
