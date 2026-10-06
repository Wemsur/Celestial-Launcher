<template>
	<div v-if="version" class="flex flex-col gap-4">
		<Card class="flex flex-col gap-3">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div class="flex min-w-0 flex-col gap-1">
					<div class="flex min-w-0 flex-wrap items-center gap-2">
						<span class="min-w-0 break-words font-semibold text-contrast">
							{{ version.versionNumber ?? version.name ?? version.id }}
						</span>
						<TagItem v-if="version.channel && version.channel !== 'release'">
							{{ version.channel }}
						</TagItem>
					</div>
					<span v-if="version.name && version.name !== version.id" class="min-w-0 break-words text-sm text-secondary">
						{{ version.name }}
					</span>
					<span v-if="version.publishedAt" class="text-sm text-secondary">
						{{ formatDate(version.publishedAt) }}
					</span>
				</div>
				<div class="flex shrink-0 items-center gap-2">
					<Button
						v-if="provider?.resolveDownload && !files.length"
						color="brand"
						:disabled="installing"
						@click="installVersion()"
					>
						<SpinnerIcon v-if="installing" class="animate-spin" />
						<DownloadIcon v-else />
						{{ formatMessage(commonMessages.installButton) }}
					</Button>
				</div>
			</div>

			<div
				v-if="files.length"
				class="flex flex-col gap-1.5 border-0 border-t border-solid border-surface-5 pt-3"
			>
				<h2 class="m-0 text-xs font-semibold uppercase text-secondary">
					{{ formatMessage(messages.files) }}
				</h2>
				<div v-for="file in files" :key="file.id" class="flex items-center justify-between gap-2">
					<span class="min-w-0 break-words text-primary">{{ file.name }}</span>
					<div class="flex shrink-0 items-center gap-2">
						<span v-if="file.size" class="text-xs text-secondary">{{ formatSize(file.size) }}</span>
						<Button size="sm" color="brand" :disabled="installing" @click="installVersion(file.id)">
							<DownloadIcon />
							{{ formatMessage(commonMessages.installButton) }}
						</Button>
					</div>
				</div>
			</div>
		</Card>

		<Card>
			<div v-if="body && isMarkdown" class="markdown-body" v-html="rendered" />
			<p v-else-if="body" class="m-0 whitespace-pre-wrap text-primary">{{ body }}</p>
			<p v-else class="m-0 text-secondary">{{ formatMessage(messages.noChangelog) }}</p>
		</Card>
	</div>

	<Card v-else>
		<p class="m-0 text-secondary">{{ formatMessage(messages.missing) }}</p>
	</Card>
</template>

<script setup lang="ts">
import { DownloadIcon, SpinnerIcon } from '@modrinth/assets'
import {
	Button,
	Card,
	commonMessages,
	defineMessages,
	injectNotificationManager,
	TagItem,
	useVIntl,
} from '@modrinth/ui'
import { renderHighlightedString } from '@modrinth/utils'
import dayjs from 'dayjs'
import { computed } from 'vue'
import { useRoute } from 'vue-router'

import { installedManifest } from '@/content-sources/catalog'
import { useSourceProject } from '@/composables/use-source-project'
import { toError } from '@/helpers/errors'

const route = useRoute()
const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()
const { versions, provider, sourceId, installing, filesOf, install } = useSourceProject()

const messages = defineMessages({
	files: { id: 'app.source-project.files', defaultMessage: 'Files' },
	noChangelog: {
		id: 'app.source-project.no-changelog',
		defaultMessage: 'This version has no changelog.',
	},
	missing: {
		id: 'app.source-project.version-missing',
		defaultMessage: 'The source does not have this version.',
	},
})

const versionId = computed(() => String(route.params.version ?? ''))

const version = computed(() =>
	versions.value.find((candidate) => candidate.id === versionId.value),
)

/** The version's body is the changelog for Modrinth, the release notes here. */
const body = computed(() => version.value?.body ?? '')

// From the installed-source list, where the backend keeps manifests — not from
// the provider, which carries only the source's own functions. See Description.
const isMarkdown = computed(
	() => (installedManifest(sourceId.value)?.detail_format ?? 'markdown') === 'markdown',
)

const rendered = computed(() => renderHighlightedString(body.value))

const files = computed(() => filesOf(version.value))

function formatDate(iso: string): string {
	const parsed = dayjs(iso)
	return parsed.isValid() ? parsed.format('YYYY-MM-DD HH:mm') : iso
}

function formatSize(bytes: number): string {
	if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
	if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`
	return `${bytes} B`
}

async function installVersion(fileId?: string) {
	const current = version.value
	if (!current) return
	try {
		await install({
			versionId: current.id,
			versionName: current.name ?? current.id,
			fileId,
		})
	} catch (caught) {
		handleError(toError(caught))
	}
}
</script>
