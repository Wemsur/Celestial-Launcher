<script setup lang="ts">
import { Admonition, Button, LoadingIndicator, NewModal as Modal, injectNotificationManager, useVIntl } from '@modrinth/ui'
import { useModalStack } from '@modrinth/ui/src/composables/modal-stack'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'

import AnnouncementArticle from './AnnouncementArticle.vue'
import {
	disposeAnnouncements, initializeAnnouncements, isAnnouncementActive, loadAnnouncementBody,
	suppressAnnouncement, updateAnnouncementClock, useAnnouncements,
} from '@/composables/use-announcements'
import { announcementMessages as messages } from '@/helpers/announcement-messages'
import { announcementList, type AnnouncementMetadata } from '@/helpers/announcements'
import { toError } from '@/helpers/errors'

const props = defineProps<{ ready: boolean }>()
const state = useAnnouncements()
const { snapshot, active, now, openRequest } = state
const { formatMessage, locale } = useVIntl()
const { handleError } = injectNotificationManager()
const { hasModal } = useModalStack()
const modal = ref<InstanceType<typeof Modal> | null>(null)
const mounted = ref(false)
const opened = ref(false)
const currentId = ref<string | null>(null)
const current = ref<AnnouncementMetadata | null>(null)
const entries = ref<AnnouncementMetadata[]>([])
const cursor = ref<number | null>(0)
const loadingPage = ref(false)
const pageError = ref<string | null>(null)
const saving = ref(false)
const sentinel = ref<HTMLElement | null>(null)
let observer: IntersectionObserver | null = null
let clock: ReturnType<typeof setInterval> | null = null
let viewEpoch = 0
let initialized = false
let autoHandled = false
let autoChecking = false

const others = computed(() => entries.value.filter((item) => item.id !== currentId.value && isAnnouncementActive(item, now.value)))
const history = computed(() => entries.value.filter((item) => item.id !== currentId.value && !isAnnouncementActive(item, now.value)))
const currentActive = computed(() => active.value.find((item) => item.id === currentId.value))
const canSuppress = computed(() => Boolean(currentActive.value && !currentActive.value.suppressed) && !saving.value)
const lastSync = computed(() => snapshot.value.last_success_at
	? new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(snapshot.value.last_success_at)) : null)

async function loadMore(): Promise<void> {
	if (loadingPage.value || cursor.value === null || !opened.value) return
	const epoch = viewEpoch
	loadingPage.value = true
	pageError.value = null
	try {
		const page = await announcementList(cursor.value)
		if (epoch !== viewEpoch) return
		const seen = new Set(entries.value.map((item) => item.id))
		entries.value.push(...page.items.filter((item) => !seen.has(item.id)))
		cursor.value = page.next_cursor
	} catch (error) {
		if (epoch === viewEpoch) pageError.value = toError(error).message
	} finally {
		if (epoch === viewEpoch) loadingPage.value = false
	}
}

async function resetContent(): Promise<void> {
	const epoch = ++viewEpoch
	entries.value = []
	current.value = null
	cursor.value = 0
	loadingPage.value = false
	if (currentId.value) {
		try {
			const body = await loadAnnouncementBody(currentId.value)
			if (epoch === viewEpoch) current.value = body.announcement
		} catch (error) {
			if (epoch === viewEpoch) {
				current.value = active.value.find((item) => item.id === currentId.value) ?? null
				pageError.value = toError(error).message
			}
		}
	}
	if (epoch === viewEpoch) await loadMore()
}

async function show(id: string | null): Promise<void> {
	if (!mounted.value) return
	currentId.value = id
	opened.value = true
	await nextTick()
	modal.value?.show()
	await resetContent()
}

function onHide(): void {
	opened.value = false
	viewEpoch++
	observer?.disconnect()
}

async function saveSuppression(): Promise<void> {
	const id = currentId.value
	if (!id || !canSuppress.value) return
	saving.value = true
	try {
		await suppressAnnouncement(id)
		saving.value = false
		await nextTick()
		modal.value?.hide()
	} catch (error) { handleError(error) }
	finally { saving.value = false }
}

watch(openRequest, () => {
	autoHandled = true
	const selected = active.value.find((item) => !item.suppressed) ?? active.value[0]
	void show(selected?.id ?? null)
})

watch(() => snapshot.value.generation, () => {
	if (opened.value) void resetContent()
})

watch(sentinel, (element) => {
	observer?.disconnect()
	if (!element) return
	observer = new IntersectionObserver((items) => {
		if (items.some((item) => item.isIntersecting) && !pageError.value) void loadMore()
	}, { root: element.closest('[data-modal-content]'), rootMargin: '0px 0px 160px 0px' })
	observer.observe(element)
}, { flush: 'post' })

watch(() => [props.ready, mounted.value], () => {
	if (props.ready && mounted.value && !initialized) {
		initialized = true
		void initializeAnnouncements()
	}
}, { immediate: true })

watch(() => [props.ready, mounted.value, hasModal.value, snapshot.value.fresh,
	snapshot.value.refreshing, snapshot.value.error, snapshot.value.candidate_id, now.value], async () => {
	if (!props.ready || !mounted.value || autoHandled || autoChecking || snapshot.value.refreshing) return
	if (!snapshot.value.fresh) {
		if (snapshot.value.error) autoHandled = true
		return
	}
	const id = snapshot.value.candidate_id
	if (!id) { autoHandled = true; return }
	if (hasModal.value) return
	autoChecking = true
	try {
		const body = await loadAnnouncementBody(id)
		if (autoHandled || !snapshot.value.fresh || hasModal.value) return
		const latest = active.value.find((item) => item.id === id && !item.suppressed)
		if (!latest || body.stale || !body.markdown || !isAnnouncementActive(body.announcement)) {
			autoHandled = true
			return
		}
		autoHandled = true
		await show(id)
	} catch (error) {
		autoHandled = true
		console.warn('Announcement could not be shown:', error)
	} finally { autoChecking = false }
})

onMounted(() => {
	mounted.value = true
	clock = setInterval(updateAnnouncementClock, 30_000)
	window.addEventListener('focus', updateAnnouncementClock)
})
onBeforeUnmount(() => {
	if (clock) clearInterval(clock)
	observer?.disconnect()
	window.removeEventListener('focus', updateAnnouncementClock)
	disposeAnnouncements()
})
</script>

<template>
	<Modal ref="modal" :header="formatMessage(messages.title)" scrollable width="48rem" :on-hide="onHide" :disable-close="saving" actions-divider>
		<div class="flex min-w-0 flex-col gap-6">
			<Admonition v-if="!snapshot.fresh && snapshot.last_success_at" type="warning" :header="formatMessage(messages.cache)">
				<p v-if="lastSync" class="m-0 text-secondary">{{ formatMessage(messages.lastSync, { date: lastSync }) }}</p>
			</Admonition>
			<section v-if="current" class="min-w-0 space-y-3">
				<h2 class="m-0 text-sm font-semibold text-secondary">{{ formatMessage(messages.current) }}</h2>
				<AnnouncementArticle :metadata="current" />
				<p v-if="currentActive?.suppressed" class="m-0 text-sm text-secondary">{{ formatMessage(messages.suppressed) }}</p>
			</section>
			<section v-if="others.length" class="min-w-0 space-y-6">
				<h2 class="m-0 text-lg font-semibold text-contrast">{{ formatMessage(messages.other) }}</h2>
				<AnnouncementArticle v-for="item in others" :key="item.id" :metadata="item" class="border-0 border-b border-solid border-surface-5 pb-6" />
			</section>
			<section v-if="history.length" class="min-w-0 space-y-6">
				<h2 class="m-0 text-lg font-semibold text-contrast">{{ formatMessage(messages.history) }}</h2>
				<AnnouncementArticle v-for="item in history" :key="item.id" :metadata="item" class="border-0 border-b border-solid border-surface-5 pb-6" />
			</section>
			<div v-if="loadingPage" class="flex items-center gap-2 text-secondary" role="status"><LoadingIndicator /> {{ formatMessage(messages.loading) }}</div>
			<Admonition v-if="pageError" type="warning" :header="formatMessage(messages.unavailable)">
				<p class="break-words">{{ pageError }}</p>
				<Button type="quiet" @click="loadMore">{{ formatMessage(messages.retry) }}</Button>
			</Admonition>
			<p v-if="!current && !entries.length && !loadingPage && !pageError" class="m-0 text-secondary">{{ formatMessage(messages.empty) }}</p>
			<div v-if="cursor !== null" ref="sentinel" class="flex justify-center">
				<Button type="quiet" :disabled="loadingPage" @click="loadMore">{{ formatMessage(messages.more) }}</Button>
			</div>
		</div>
		<template #actions>
			<div class="flex justify-end gap-2">
				<Button :disabled="!canSuppress" @click="saveSuppression">{{ formatMessage(messages.suppress) }}</Button>
				<Button color="brand" :disabled="saving" @click="modal?.hide()">{{ formatMessage(messages.close) }}</Button>
			</div>
		</template>
	</Modal>
</template>
