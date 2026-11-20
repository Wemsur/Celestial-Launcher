import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { computed, reactive, ref } from 'vue'

import {
	announcementGet, announcementRefresh, announcementSnapshot, announcementSuppress,
	type AnnouncementBody, type AnnouncementMetadata, type AnnouncementSnapshot,
} from '@/helpers/announcements'
import { toError } from '@/helpers/errors'

const snapshot = ref<AnnouncementSnapshot>({
	generation: null, last_success_at: null, refreshing: false, fresh: false, error: null,
	active: [], visible_count: 0, candidate_id: null,
})
const now = ref(Date.now())
const openRequest = ref(0)
const bodies = reactive(new Map<string, { loading: boolean; response: AnnouncementBody | null; error: string | null }>())
const pendingBodies = new Map<string, Promise<AnnouncementBody>>()
let bodyEpoch = 0
let queryQueue: Promise<void> = Promise.resolve()
let initialize: Promise<void> | null = null
let unlisten: UnlistenFn | null = null
let session = 0

export function isAnnouncementActive(item: AnnouncementMetadata, time = Date.now()): boolean {
	return item.state === 'published' && item.starts_at !== null && item.expires_at !== null
		&& Date.parse(item.starts_at) <= time && time < Date.parse(item.expires_at)
}

export function reloadAnnouncements(): Promise<void> {
	const query = queryQueue.then(async () => {
		const next = await announcementSnapshot()
		if (next.generation !== snapshot.value.generation) {
			bodyEpoch++
			bodies.clear()
		}
		snapshot.value = next
		now.value = Date.now()
	})
	queryQueue = query.catch(() => {})
	return query
}

export function initializeAnnouncements(): Promise<void> {
	if (initialize) return initialize
	const token = ++session
	initialize = (async () => {
		const off = await listen('announcements-updated', () => {
			void reloadAnnouncements().catch((error) => console.warn('Announcement snapshot unavailable:', error))
		})
		if (token !== session) { off(); return }
		unlisten = off
		await reloadAnnouncements()
		await announcementRefresh()
		await reloadAnnouncements()
	})().catch((error) => {
		snapshot.value.error = toError(error).message
		snapshot.value.fresh = false
		console.warn('Announcements are unavailable:', error)
	})
	return initialize
}

export function disposeAnnouncements(): void {
	session++
	unlisten?.()
	unlisten = null
	initialize = null
}

export function updateAnnouncementClock(): void {
	now.value = Date.now()
	if (initialize) void reloadAnnouncements().catch((error) => console.warn('Announcement clock refresh failed:', error))
}

export function loadAnnouncementBody(id: string, force = false): Promise<AnnouncementBody> {
	const epoch = bodyEpoch
	const key = `${epoch}:${id}`
	const pending = pendingBodies.get(key)
	if (pending) return pending
	const stored = bodies.get(id)?.response
	if (stored && !force) return Promise.resolve(stored)
	bodies.set(id, { loading: true, response: null, error: null })
	const request = announcementGet(id).then((response) => {
		if (epoch === bodyEpoch) bodies.set(id, { loading: false, response, error: response.error })
		return response
	}).catch((error) => {
		if (epoch === bodyEpoch) bodies.set(id, { loading: false, response: null, error: toError(error).message })
		throw error
	}).finally(() => pendingBodies.delete(key))
	pendingBodies.set(key, request)
	return request
}

export async function suppressAnnouncement(id: string): Promise<void> {
	await announcementSuppress(id)
	for (const item of snapshot.value.active) if (item.id === id) item.suppressed = true
	const body = bodies.get(id)?.response
	if (body) body.announcement.suppressed = true
	snapshot.value.candidate_id = snapshot.value.active.find((item) => !item.suppressed && isAnnouncementActive(item))?.id ?? null
	void reloadAnnouncements().catch((error) => console.warn('Announcement preference refresh failed:', error))
}

export function requestAnnouncements(): void {
	openRequest.value++
}

export function useAnnouncements() {
	const active = computed(() => snapshot.value.active.filter((item) => isAnnouncementActive(item, now.value)))
	return { snapshot, active, now, openRequest, bodies }
}
