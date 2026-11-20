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
	<article class="min-w-0 space-y-3" :data-announcement-id="metadata.id">
		<div class="flex flex-wrap items-center gap-2">
			<h3 class="m-0 min-w-0 break-words text-lg font-semibold text-contrast">
				{{ metadata.title ?? formatMessage(messages.withdrawn) }}
			</h3>
			<Badge :type="formatMessage(status)" :color="isAnnouncementActive(metadata, now) ? 'green' : 'gray'" />
			<Badge
				v-if="metadata.level && metadata.state !== 'withdrawn'"
				:type="formatMessage(messages[metadata.level])"
				:color="metadata.level === 'critical' ? 'red' : metadata.level === 'warning' ? 'orange' : 'gray'"
			/>
		</div>
		<p class="m-0 text-xs text-secondary">{{ formatMessage(messages.published, { date: date(metadata.published_at) }) }}</p>
		<p v-if="metadata.starts_at && metadata.expires_at" class="m-0 text-xs text-secondary">
			{{ formatMessage(messages.period, { start: date(metadata.starts_at), end: date(metadata.expires_at) }) }}
		</p>
		<p v-if="body?.response?.stale" class="m-0 text-sm text-secondary">{{ formatMessage(messages.cachedBody) }}</p>
		<template v-if="metadata.state !== 'withdrawn'">
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
		</template>
	</article>
</template>
