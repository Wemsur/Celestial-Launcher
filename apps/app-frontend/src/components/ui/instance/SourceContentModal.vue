<script setup lang="ts">
import {
	CheckIcon,
	CircleAlertIcon,
	DownloadIcon,
	RefreshCwIcon,
	TrashIcon,
} from '@modrinth/assets'
import { Button, defineMessages, injectNotificationManager, NewModal, useVIntl } from '@modrinth/ui'
import { computed, ref } from 'vue'

import { selectableSources } from '@/content-sources/catalog'
import {
	forgetInstalledSourceContent,
	installSourceContent,
	listInstalledSourceContent,
	type SourceContentRecord,
} from '@/content-sources/ipc'
import { sourceProvider } from '@/content-sources/registry'
import type { ContentType, ContentVersion, UpdateQueryItem } from '@/content-sources/types'
import { toError } from '@/helpers/errors'

const props = defineProps<{
	instanceId: string
	gameVersion?: string
	loader?: string
}>()

const { formatMessage } = useVIntl()
const { handleError, addNotification } = injectNotificationManager()

const messages = defineMessages({
	title: { id: 'app.source-content.title', defaultMessage: 'Content sources' },
	description: {
		id: 'app.source-content.description',
		defaultMessage:
			'Content installed from a content source, and whether its source has something newer.',
	},
	empty: {
		id: 'app.source-content.empty',
		defaultMessage:
			'Nothing has been installed from a content source into this instance yet.',
	},
	check: { id: 'app.source-content.check', defaultMessage: 'Check for updates' },
	checking: { id: 'app.source-content.checking', defaultMessage: 'Checking…' },
	update: { id: 'app.source-content.update', defaultMessage: 'Update' },
	upToDate: { id: 'app.source-content.up-to-date', defaultMessage: 'Up to date' },
	hasUpdate: { id: 'app.source-content.has-update', defaultMessage: 'Update available' },
	noCheck: {
		id: 'app.source-content.no-check',
		defaultMessage: 'This source cannot check for updates.',
	},
	failed: { id: 'app.source-content.failed', defaultMessage: 'Check failed' },
	openVersions: { id: 'app.source-content.versions', defaultMessage: 'Change version' },
	reDownload: { id: 'app.source-content.redownload', defaultMessage: 'Download again' },
	worldNote: {
		id: 'app.source-content.world-note',
		defaultMessage:
			'Worlds are never replaced — each download makes a new copy, so your edits are safe. That also means there is nothing to update.',
	},
	forget: { id: 'app.source-content.forget', defaultMessage: 'Forget' },
	forgetHint: {
		id: 'app.source-content.forget-hint',
		defaultMessage: 'Stops tracking this file here. The file itself is left in place.',
	},
	done: { id: 'app.source-content.done', defaultMessage: 'Done' },
	unknownSource: {
		id: 'app.source-content.unknown-source',
		defaultMessage: 'Source not loaded',
	},
})

const modal = ref<InstanceType<typeof NewModal> | null>(null)
const records = ref<SourceContentRecord[]>([])
const loading = ref(false)
const checking = ref(false)
const busyId = ref<string | null>(null)

/** Per-record check state, filled in as each source answers. */
type CheckState =
	| { status: 'ok'; latestVersionId?: string; latestVersionName?: string }
	| { status: 'unsupported' }
	| { status: 'error'; message: string }

const checks = ref<Record<string, CheckState>>({})

/** Versions offered for a record whose version picker is open. */
const versionLists = ref<Record<string, ContentVersion[]>>({})
const openVersionsFor = ref<string | null>(null)

const sourceNames = computed(
	() => new Map(selectableSources().map((source) => [source.id, source.name])),
)

function sourceNameOf(record: SourceContentRecord): string {
	return sourceNames.value.get(record.source_id) ?? record.source_id
}

async function reload() {
	loading.value = true
	try {
		records.value = await listInstalledSourceContent(props.instanceId)
	} catch (error) {
		handleError(toError(error))
	} finally {
		loading.value = false
	}
}

async function show() {
	checks.value = {}
	versionLists.value = {}
	openVersionsFor.value = null
	modal.value?.show()
	await reload()
}

function hide() {
	modal.value?.hide()
}

/**
 * Ask every source that has installed content here whether anything is newer.
 *
 * Each source is asked once and its answers are applied as soon as they arrive,
 * so a slow source does not hold up the ones that already replied.
 */
async function checkForUpdates() {
	checking.value = true
	checks.value = {}

	const bySource = new Map<string, SourceContentRecord[]>()
	for (const record of records.value) {
		// A world is never replaced on disk, so "is there a newer version" has no
		// meaning for it: the way to get a newer one is to download it again.
		if (isWorld(record)) continue
		const list = bySource.get(record.source_id)
		if (list) {
			list.push(record)
		} else {
			bySource.set(record.source_id, [record])
		}
	}

	await Promise.all(
		[...bySource.entries()].map(async ([sourceId, items]) => {
			const provider = sourceProvider(sourceId)
			if (!provider?.checkUpdate) {
				for (const item of items) {
					checks.value = { ...checks.value, [item.id]: { status: 'unsupported' } }
				}
				return
			}

			try {
				const query: UpdateQueryItem[] = items.map((item) => ({
					id: item.project_id,
					contentType: item.content_type as ContentType,
					currentVersionId: item.version_id ?? undefined,
					gameVersion: props.gameVersion,
					loader: props.loader,
					installRule: (item.install_rule as Record<string, unknown> | null) ?? undefined,
				}))
				const results = await provider.checkUpdate(query)
				const byProject = new Map(results.map((result) => [result.id, result]))
				const next = { ...checks.value }
				for (const item of items) {
					const result = byProject.get(item.project_id)
					next[item.id] = { status: 'ok', ...result }
				}
				checks.value = next
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error)
				const next = { ...checks.value }
				for (const item of items) {
					next[item.id] = { status: 'error', message }
				}
				checks.value = next
			}
		}),
	)

	checking.value = false
}

async function applyVersion(record: SourceContentRecord, versionId?: string) {
	const provider = sourceProvider(record.source_id)
	if (!provider?.resolveDownload) {
		handleError(new Error('该内容源不支持直接下载。'))
		return
	}

	busyId.value = record.id
	try {
		const descriptor = await provider.resolveDownload(record.project_id, versionId)
		await installSourceContent(props.instanceId, record.content_type, descriptor, {
			sourceId: record.source_id,
			projectId: record.project_id,
			versionId,
			name: record.name,
			iconUrl: record.icon_url ?? undefined,
			installRule: (record.install_rule as Record<string, unknown> | null) ?? undefined,
		})
		addNotification({
			title: isWorld(record)
				? `已下载「${record.name}」的新副本`
				: `已更新「${record.name}」`,
			text: isWorld(record)
				? `来自内容源「${sourceNameOf(record)}」，在 saves 里新建了一份`
				: `来自内容源「${sourceNameOf(record)}」`,
			type: 'success',
		})
		openVersionsFor.value = null
		await reload()
		// Whatever was checked is now stale.
		checks.value = {}
	} catch (error) {
		handleError(toError(error))
	} finally {
		busyId.value = null
	}
}

async function toggleVersions(record: SourceContentRecord) {
	if (openVersionsFor.value === record.id) {
		openVersionsFor.value = null
		return
	}
	const provider = sourceProvider(record.source_id)
	if (!provider?.versions) {
		handleError(new Error('该内容源不提供版本列表。'))
		return
	}
	busyId.value = record.id
	try {
		versionLists.value = {
			...versionLists.value,
			[record.id]: await provider.versions(record.project_id, {
				contentType: record.content_type as ContentType,
				sort: 'newest',
				page: 1,
				limit: 50,
			}),
		}
		openVersionsFor.value = record.id
	} catch (error) {
		handleError(toError(error))
	} finally {
		busyId.value = null
	}
}

async function forget(record: SourceContentRecord) {
	busyId.value = record.id
	try {
		await forgetInstalledSourceContent(props.instanceId, record.id)
		await reload()
	} catch (error) {
		handleError(toError(error))
	} finally {
		busyId.value = null
	}
}

/**
 * The template cannot narrow a union returned from a call, so the state is
 * exposed as a plain string plus two accessors rather than as the union itself.
 */
type CheckStatus = 'none' | 'current' | 'outdated' | 'unsupported' | 'error'

function statusOf(record: SourceContentRecord): CheckStatus {
	const check = checks.value[record.id]
	if (!check) return 'none'
	if (check.status === 'unsupported') return 'unsupported'
	if (check.status === 'error') return 'error'
	return check.latestVersionId ? 'outdated' : 'current'
}

function latestOf(record: SourceContentRecord): string | undefined {
	const check = checks.value[record.id]
	return check && check.status === 'ok' ? check.latestVersionId : undefined
}

function errorOf(record: SourceContentRecord): string {
	const check = checks.value[record.id]
	return check && check.status === 'error' ? check.message : ''
}

/** Worlds are installed fresh every time and are never replaced in place. */
function isWorld(record: SourceContentRecord): boolean {
	return record.content_type === 'world'
}

defineExpose({ show, hide })
</script>

<template>
	<NewModal ref="modal" :header="formatMessage(messages.title)" max-width="min(720px, 92vw)">
		<div class="flex w-full max-w-[680px] flex-col gap-4">
			<p class="m-0 text-sm text-secondary">{{ formatMessage(messages.description) }}</p>

			<div class="flex items-center gap-2">
				<Button :disabled="checking || records.length === 0" @click="checkForUpdates">
					<RefreshCwIcon :class="checking ? 'animate-spin' : ''" />
					{{ formatMessage(checking ? messages.checking : messages.check) }}
				</Button>
			</div>

			<p v-if="loading" class="m-0 text-secondary">…</p>
			<p v-else-if="records.length === 0" class="m-0 text-secondary">
				{{ formatMessage(messages.empty) }}
			</p>

			<div v-else class="flex flex-col gap-3">
				<article
					v-for="record in records"
					:key="record.id"
					class="flex flex-col gap-2 rounded-xl border border-solid border-surface-5 bg-surface-3 p-4"
				>
					<header class="flex items-start justify-between gap-4">
						<div class="flex min-w-0 items-center gap-3">
							<img
								v-if="record.icon_url"
								:src="record.icon_url"
								:alt="record.name"
								class="h-10 w-10 shrink-0 rounded-lg border border-solid border-surface-5 object-cover"
							/>
							<div class="flex min-w-0 flex-col">
								<span class="truncate font-semibold text-contrast">{{ record.name }}</span>
								<span class="truncate text-xs text-secondary">
									{{ sourceNameOf(record) }} · {{ record.relative_path }}
								</span>
							</div>
						</div>

						<span
							v-if="statusOf(record) === 'current'"
							class="inline-flex shrink-0 items-center gap-1 text-xs font-semibold text-green"
						>
							<CheckIcon class="size-4" />
							{{ formatMessage(messages.upToDate) }}
						</span>
						<span
							v-else-if="statusOf(record) === 'outdated'"
							class="inline-flex shrink-0 items-center gap-1 text-xs font-semibold text-brand"
						>
							{{ formatMessage(messages.hasUpdate) }}
						</span>
						<span
							v-else-if="statusOf(record) === 'unsupported'"
							class="inline-flex shrink-0 items-center gap-1 text-xs text-secondary"
						>
							<CircleAlertIcon class="size-4" />
							{{ formatMessage(messages.noCheck) }}
						</span>
						<span
							v-else-if="statusOf(record) === 'error'"
							class="inline-flex shrink-0 items-center gap-1 text-xs text-red"
							:title="errorOf(record)"
						>
							<CircleAlertIcon class="size-4" />
							{{ formatMessage(messages.failed) }}
						</span>
						<span
							v-else-if="isWorld(record)"
							class="max-w-[22rem] shrink-0 text-right text-xs text-secondary"
						>
							{{ formatMessage(messages.worldNote) }}
						</span>
					</header>

					<footer class="flex flex-wrap items-center gap-2">
						<Button
							v-if="isWorld(record)"
							size="sm"
							:disabled="busyId === record.id"
							@click="applyVersion(record)"
						>
							<DownloadIcon />
							{{ formatMessage(messages.reDownload) }}
						</Button>
						<Button
							v-if="statusOf(record) === 'outdated'"
							size="sm"
							color="brand"
							:disabled="busyId === record.id"
							@click="applyVersion(record, latestOf(record))"
						>
							<DownloadIcon />
							{{ formatMessage(messages.update) }}
						</Button>
						<Button
							size="sm"
							type="outlined"
							:disabled="busyId === record.id"
							@click="toggleVersions(record)"
						>
							{{ formatMessage(messages.openVersions) }}
						</Button>
						<Button
							v-tooltip="formatMessage(messages.forgetHint)"
							size="sm"
							type="outlined"
							:disabled="busyId === record.id"
							@click="forget(record)"
						>
							<TrashIcon />
							{{ formatMessage(messages.forget) }}
						</Button>
					</footer>

					<div v-if="openVersionsFor === record.id" class="flex flex-wrap items-center gap-2">
						<select
							class="min-w-0 grow rounded-lg border border-solid border-surface-5 bg-surface-1 px-3 py-2 text-sm text-primary"
							@change="
								(event) =>
									applyVersion(record, (event.target as HTMLSelectElement).value)
							"
						>
							<option value="">
								{{ formatMessage(messages.openVersions) }}…
							</option>
							<option
								v-for="version in versionLists[record.id] ?? []"
								:key="version.id"
								:value="version.id"
							>
								{{ version.versionNumber ?? version.name ?? version.id }}
							</option>
						</select>
					</div>
				</article>
			</div>

			<div class="flex justify-end pt-2">
				<Button @click="hide">{{ formatMessage(messages.done) }}</Button>
			</div>
		</div>
	</NewModal>
</template>
