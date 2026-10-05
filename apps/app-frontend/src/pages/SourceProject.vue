<script setup lang="ts">
import { ExternalIcon, PlusIcon, SpinnerIcon, UndoIcon } from '@modrinth/assets'
import { Button, defineMessages, injectNotificationManager, useVIntl } from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useQueryClient } from '@tanstack/vue-query'
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { selectableSources } from '@/content-sources/catalog'
import { installSourceContent } from '@/content-sources/ipc'
import { sourceProvider } from '@/content-sources/registry'
import type { ContentProject, ContentType, ContentVersion } from '@/content-sources/types'
import { toError } from '@/helpers/errors'

const route = useRoute()
const router = useRouter()
const queryClient = useQueryClient()
const { formatMessage } = useVIntl()
const { handleError, addNotification } = injectNotificationManager()

const messages = defineMessages({
	back: { id: 'app.source-project.back', defaultMessage: 'Back' },
	loading: { id: 'app.source-project.loading', defaultMessage: 'Loading project…' },
	install: { id: 'app.source-project.install', defaultMessage: 'Install' },
	installing: { id: 'app.source-project.installing', defaultMessage: 'Installing' },
	openOnSource: { id: 'app.source-project.open', defaultMessage: 'Open on the source' },
	noDetail: {
		id: 'app.source-project.no-detail',
		defaultMessage: 'This source does not provide a project page.',
	},
	noDetailHint: {
		id: 'app.source-project.no-detail-hint',
		defaultMessage: 'You can still install it, or open it on the source itself.',
	},
	noDownload: {
		id: 'app.source-project.no-download',
		defaultMessage: 'This source cannot download this item directly.',
	},
	needInstance: {
		id: 'app.source-project.need-instance',
		defaultMessage: 'Choose an instance first, then install this item.',
	},
	unknownSource: {
		id: 'app.source-project.unknown-source',
		defaultMessage: 'That content source is not loaded.',
	},
	versions: { id: 'app.source-project.versions', defaultMessage: 'Versions' },
	version: { id: 'app.source-project.version', defaultMessage: 'Version' },
	by: { id: 'app.source-project.by', defaultMessage: 'by' },
})

const provider = computed(() => sourceProvider(sourceId.value))

const sourceId = computed(() => String(route.params.sourceId ?? ''))
const projectId = computed(() => String(route.params.projectId ?? ''))
// Split route carries the type in the path (`:projectType`); the full-width
// route carries it in the query. Fall back to the source's sole type, then mod.
const contentType = computed(
	() =>
		String(
			route.params.projectType ??
				route.query.type ??
				provider.value?.manifest.content_types[0] ??
				'mod',
		) as ContentType,
)

const sourceName = computed(
	() =>
		selectableSources().find((source) => source.id === sourceId.value)?.name ??
		sourceId.value,
)

const project = ref<ContentProject | null>(null)
const versions = ref<ContentVersion[]>([])
const selectedVersionId = ref<string | undefined>(undefined)
const loading = ref(true)
const loadError = ref<string | null>(null)
const installing = ref(false)

/** The instance this browse session is installing into, when there is one. */
const instanceId = computed(() =>
	route.query.i ? String(route.query.i) : undefined,
)

/** A skin belongs to the launcher; everything else needs an instance. */
const needsInstance = computed(() => contentType.value !== 'skin')
const blockedNoInstance = computed(
	() => needsInstance.value && !instanceId.value,
)

function openLink(url: string) {
	void openUrl(url)
}

async function load() {
	loading.value = true
	loadError.value = null
	project.value = null
	versions.value = []

	const current = provider.value
	if (!current) {
		loadError.value = formatMessage(messages.unknownSource)
		loading.value = false
		return
	}

	try {
		// Cached like the browse list: a source has no backend cache of its own,
		// so without this every visit to a project is another round trip.
		const data = await queryClient.fetchQuery({
			queryKey: [
				'content-source',
				'project',
				sourceId.value,
				projectId.value,
				contentType.value,
			],
			staleTime: 120_000,
			queryFn: async () => ({
				project: current.detail
					? await current.detail(projectId.value, contentType.value)
					: null,
				versions: current.versions
					? await current.versions(projectId.value, {
							contentType: contentType.value,
							sort: 'newest',
							page: 1,
							limit: 50,
						})
					: [],
			}),
		})
		project.value = data.project
		versions.value = data.versions
		selectedVersionId.value = data.versions[0]?.id
	} catch (error) {
		loadError.value = toError(error).message
	} finally {
		loading.value = false
	}
}

onMounted(load)
watch(() => [sourceId.value, projectId.value, contentType.value], load)

async function install() {
	const current = provider.value
	if (!current?.resolveDownload) {
		handleError(new Error(formatMessage(messages.noDownload)))
		return
	}
	if (blockedNoInstance.value) {
		handleError(new Error(formatMessage(messages.needInstance)))
		return
	}

	installing.value = true
	try {
		const descriptor = await current.resolveDownload(
			projectId.value,
			selectedVersionId.value,
		)
		const installed = await installSourceContent(
			needsInstance.value ? instanceId.value : undefined,
			contentType.value,
			descriptor,
		)
		addNotification({
			title: `已下载「${project.value?.name ?? installed.file_name}」`,
			text: `来自内容源「${sourceName.value}」，位置：${installed.relative_path}`,
			type: 'success',
		})
	} catch (error) {
		handleError(error instanceof Error ? error : new Error(String(error)))
	} finally {
		installing.value = false
	}
}
</script>

<template>
	<div class="flex flex-col gap-6 p-6">
		<div class="flex items-center gap-2">
			<Button size="sm" type="outlined" @click="router.back()">
				<UndoIcon />
				{{ formatMessage(messages.back) }}
			</Button>
			<span class="text-sm text-secondary">{{ sourceName }}</span>
		</div>

		<p v-if="loading" class="m-0 text-secondary">{{ formatMessage(messages.loading) }}</p>

		<template v-else>
			<div v-if="loadError" class="flex items-start gap-2 text-sm text-red">
				<span class="break-all">{{ loadError }}</span>
			</div>

			<header class="flex items-start gap-4">
				<img
					v-if="project?.iconUrl"
					:src="project.iconUrl"
					:alt="project.name"
					class="h-20 w-20 shrink-0 rounded-xl border border-solid border-surface-5 object-cover"
				/>
				<div class="flex min-w-0 flex-col gap-1">
					<h1 class="m-0 text-2xl font-semibold text-contrast">
						{{ project?.name ?? projectId }}
					</h1>
					<p v-if="project?.author" class="m-0 text-secondary">
						{{ formatMessage(messages.by) }} {{ project.author }}
					</p>
					<p v-if="project?.summary" class="m-0 text-primary">{{ project.summary }}</p>
					<div v-if="project?.tags?.length" class="mt-1 flex flex-wrap gap-2">
						<span
							v-for="tag in project.tags"
							:key="tag"
							class="inline-flex items-center rounded-full border border-solid border-surface-5 bg-surface-1 px-2.5 py-1 text-xs text-secondary"
						>
							{{ tag }}
						</span>
					</div>
				</div>
			</header>

			<p v-if="blockedNoInstance && provider?.resolveDownload" class="m-0 text-sm text-secondary">
				{{ formatMessage(messages.needInstance) }}
			</p>

			<div class="flex flex-wrap items-center gap-2">
				<Button
					v-if="provider?.resolveDownload"
					color="brand"
					:disabled="installing || blockedNoInstance"
					@click="install"
				>
					<SpinnerIcon v-if="installing" class="animate-spin" />
					<PlusIcon v-else />
					{{
						formatMessage(installing ? messages.installing : messages.install)
					}}
				</Button>
				<Button
					v-for="link in project?.links ?? []"
					:key="link.url"
					type="outlined"
					@click="openLink(link.url)"
				>
					<ExternalIcon />
					{{ link.label }}
				</Button>
			</div>

			<div
				v-if="versions.length"
				class="flex max-w-md flex-col gap-2 rounded-xl border border-solid border-surface-5 bg-surface-3 p-4"
			>
				<h2 class="m-0 text-sm font-semibold text-contrast">
					{{ formatMessage(messages.versions) }}
				</h2>
				<label class="flex items-center gap-2">
					<span class="text-sm text-secondary">{{ formatMessage(messages.version) }}</span>
					<select
						v-model="selectedVersionId"
						class="min-w-0 grow rounded-lg border border-solid border-surface-5 bg-surface-1 px-3 py-2 text-sm text-primary"
					>
						<option v-for="version in versions" :key="version.id" :value="version.id">
							{{ version.versionNumber ?? version.name ?? version.id }}
						</option>
					</select>
				</label>
			</div>

			<div
				v-if="!project && !loadError"
				class="flex flex-col gap-1 rounded-xl border border-solid border-surface-5 bg-surface-3 p-4"
			>
				<span class="font-semibold text-contrast">{{ formatMessage(messages.noDetail) }}</span>
				<span class="text-sm text-secondary">{{ formatMessage(messages.noDetailHint) }}</span>
			</div>

			<article
				v-if="project?.body"
				class="whitespace-pre-wrap break-words text-base leading-relaxed text-primary"
			>
				{{ project.body }}
			</article>

			<div v-if="project?.gallery?.length" class="grid grid-cols-2 gap-3 md:grid-cols-3">
				<img
					v-for="image in project.gallery"
					:key="image.url"
					:src="image.url"
					:alt="image.title ?? ''"
					class="w-full rounded-xl border border-solid border-surface-5 object-cover"
				/>
			</div>
		</template>
	</div>
</template>
