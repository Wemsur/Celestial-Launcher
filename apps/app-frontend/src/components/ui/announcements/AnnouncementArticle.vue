<script setup lang="ts">
import { Admonition, Badge, Button, LoadingIndicator, useVIntl } from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed, watch } from 'vue'

import { isAnnouncementActive, loadAnnouncementBody, useAnnouncements } from '@/composables/use-announcements'
import { announcementMessages as messages } from '@/helpers/announcement-messages'
import { announcementLink, renderAnnouncementMarkdown } from '@/helpers/announcement-markdown'
import type { AnnouncementMetadata } from '@/helpers/announcements'

const props = defineProps<{ metadata: AnnouncementMetadata }>()
const { bodies, now } = useAnnouncements()
const { formatMessage, locale } = useVIntl()
const body = computed(() => bodies.get(props.metadata.id))
const metadata = computed(() => props.metadata.state === 'withdrawn' ? props.metadata
	: body.value?.response?.stale ? body.value.response.announcement : props.metadata)
const html = computed(() => metadata.value.state === 'withdrawn' ? ''
	: renderAnnouncementMarkdown(body.value?.response?.markdown ?? ''))
const status = computed(() => metadata.value.state === 'withdrawn' ? messages.withdrawn
	: metadata.value.state === 'ended' ? messages.ended
		: isAnnouncementActive(metadata.value, now.value) ? messages.active : messages.expired)
const dateFormatter = computed(() => new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }))
const date = (value: string) => dateFormatter.value.format(new Date(value))

async function retry(): Promise<void> {
	try { await loadAnnouncementBody(props.metadata.id, true) }
	catch (error) { console.warn('Announcement body is unavailable:', error) }
}

watch(() => [props.metadata.id, props.metadata.body_sha256], () => {
	if (props.metadata.state !== 'withdrawn') {
		void loadAnnouncementBody(props.metadata.id).catch((error) => console.warn('Announcement body is unavailable:', error))
	}
}, { immediate: true })

async function openLink(event: MouseEvent): Promise<void> {
	const target = event.target instanceof Element ? event.target.closest('a') : null
	if (!target) return
	event.preventDefault()
	const href = announcementLink(target.getAttribute('href'))
	if (!href) return
	try { await openUrl(href) }
	catch (error) { console.warn('Announcement link could not be opened:', error) }
}
</script>

<template>
	<article
		v-if="metadata.state !== 'withdrawn'"
		class="announcement-card min-w-0 overflow-hidden rounded-xl"
		:data-announcement-id="metadata.id"
		:data-level="metadata.level"
	>
		<header class="announcement-header flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3 sm:px-5">
			<h3 class="m-0 min-w-0 break-words text-lg font-semibold text-contrast">
				{{ metadata.title ?? formatMessage(messages.withdrawn) }}
			</h3>
			<Badge :type="formatMessage(status)" :color="isAnnouncementActive(metadata, now) ? 'green' : 'gray'" />
			<Badge
				v-if="metadata.level"
				:type="formatMessage(messages[metadata.level])"
				:color="metadata.level === 'critical' ? 'red' : metadata.level === 'warning' ? 'orange' : 'gray'"
			/>
			<span class="text-xs text-secondary">{{ formatMessage(messages.published, { date: date(metadata.published_at) }) }}</span>
			<span v-if="metadata.expires_at" class="text-xs text-secondary">
				{{ formatMessage(messages.expires, { date: date(metadata.expires_at) }) }}
			</span>
		</header>
		<div class="min-w-0 space-y-3 p-4 sm:p-5">
			<p v-if="body?.response?.stale" class="m-0 text-sm text-secondary">{{ formatMessage(messages.cachedBody) }}</p>
			<div v-if="body?.loading" class="flex items-center gap-2 text-secondary" role="status">
				<LoadingIndicator /> {{ formatMessage(messages.loading) }}
			</div>
			<div
				v-else-if="body?.response?.markdown"
				class="markdown-body min-w-0 break-words text-primary"
				@click="openLink"
				@auxclick="openLink"
				v-html="html"
			/>
			<Admonition v-else type="warning" :header="formatMessage(messages.unavailable)">
				<p v-if="body?.error" class="break-words text-secondary">{{ body.error }}</p>
				<Button type="quiet" @click="retry">{{ formatMessage(messages.retry) }}</Button>
			</Admonition>
		</div>
	</article>
</template>

<style scoped>
.announcement-card {
	--announcement-base: color-mix(in srgb, var(--surface-5) 95%, var(--color-green-50) 5%);
	--announcement-tint: var(--announcement-base);
	background-color: color-mix(in srgb, var(--surface-5) 75%, transparent);
	background-color: color-mix(in srgb, var(--announcement-tint) 75%, transparent);
}

.announcement-header {
	background-color: color-mix(in srgb, var(--surface-4) 95%, var(--color-green-50) 5%);
}

.announcement-card[data-level='critical'] {
	--announcement-tint: color-mix(in srgb, var(--announcement-base) 98%, var(--color-red) 2%);
}

.announcement-card[data-level='warning'] {
	--announcement-tint: color-mix(in srgb, var(--announcement-base) 96%, var(--color-orange) 4%);
}
</style>
