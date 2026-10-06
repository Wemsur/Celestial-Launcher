<template>
	<div class="flex flex-col">
		<Card v-for="version in versions" :key="version.id">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div class="flex min-w-0 flex-col gap-1">
					<div class="flex flex-wrap items-center gap-2">
						<span class="font-semibold text-contrast">
							{{ version.versionNumber ?? version.name ?? version.id }}
						</span>
						<TagItem v-if="version.channel && version.channel !== 'release'">
							{{ version.channel }}
						</TagItem>
					</div>
					<span v-if="version.name && version.name !== version.id" class="text-sm text-secondary">
						{{ version.name }}
					</span>
					<span v-if="version.publishedAt" class="text-sm text-secondary">
						{{ formatDate(version.publishedAt) }}
					</span>
				</div>

				<div class="flex shrink-0 items-center gap-2">
					<Button
						size="sm"
						type="outlined"
						@click="router.push(versionHref(version))"
					>
						{{ formatMessage(messages.view) }}
					</Button>
					<Button
						v-if="provider?.resolveDownload"
						size="sm"
						color="brand"
						:disabled="installing"
						@click="installVersion(version)"
					>
						<DownloadIcon />
						{{ formatMessage(commonMessages.installButton) }}
					</Button>
				</div>
			</div>
		</Card>

		<Card v-if="versions.length === 0">
			<p class="m-0 text-secondary">{{ formatMessage(messages.empty) }}</p>
		</Card>
	</div>
</template>

<script setup lang="ts">
import { DownloadIcon } from '@modrinth/assets'
import {
	Button,
	Card,
	commonMessages,
	defineMessages,
	injectNotificationManager,
	TagItem,
	useVIntl,
} from '@modrinth/ui'
import dayjs from 'dayjs'
import { useRoute, useRouter } from 'vue-router'

import { sourceProjectBasePathOf } from '@/composables/use-split-view.ts'
import { useSourceProject } from '@/composables/use-source-project'
import { toError } from '@/helpers/errors'
import type { ContentVersion } from '@/content-sources/types'

const route = useRoute()
const router = useRouter()
const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()
const { versions, provider, installing, install } = useSourceProject()

const messages = defineMessages({
	empty: {
		id: 'app.source-project.no-versions',
		defaultMessage: 'This source does not list versions.',
	},
	view: { id: 'app.source-project.view-version', defaultMessage: 'Details' },
})

function versionHref(version: ContentVersion) {
	return `${sourceProjectBasePathOf(route)}/version/${encodeURIComponent(version.id)}`
}

function formatDate(iso: string): string {
	const parsed = dayjs(iso)
	return parsed.isValid() ? parsed.format('YYYY-MM-DD HH:mm') : iso
}

async function installVersion(version: ContentVersion) {
	try {
		await install({ versionId: version.id, versionName: version.name ?? version.id })
	} catch (caught) {
		handleError(toError(caught))
	}
}
</script>
