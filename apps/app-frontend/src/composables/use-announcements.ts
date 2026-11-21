import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { computed, reactive, ref } from 'vue'

import {
	announcementGet, announcementRefresh, announcementSnapshot, announcementSuppress,
	type AnnouncementBody, type AnnouncementMetadata, type AnnouncementSnapshot,
} from '@/helpers/announcements'
import { toError } from '@/helpers/errors'

console.info('[announcements] 公告前端模块已加载')

const snapshot = ref<AnnouncementSnapshot>({
	generation: null, source: null, last_success_at: null, refreshing: false, fresh: false, error: null,
	active: [], total_count: 0, visible_count: 0, candidate_id: null,
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
let lastSnapshotLog = ''
let refreshStartedAt: number | null = null
let warnedOldBackend = false

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
		const summary = {
			source: next.source ?? '本地缓存/尚未确认', generation: next.generation,
			total: next.total_count ?? next.visible_count, visible: next.visible_count,
			active: next.active.length, candidate: next.candidate_id,
			refreshing: next.refreshing, fresh: next.fresh, error: next.error,
			activeItems: next.active.slice(0, 20).map((item) => ({
				id: item.id, starts: item.starts_at, expires: item.expires_at,
				suppressed: item.suppressed, bodyCached: item.body_cached,
			})),
		}
		const signature = JSON.stringify(summary)
		if (signature !== lastSnapshotLog) {
			lastSnapshotLog = signature
			console.info('[announcements] 公告状态', summary)
			if (next.error) console.warn('[announcements] 后台获取公告失败:', next.error)
		}
		if (!warnedOldBackend && (typeof next.total_count !== 'number' || !('source' in next))) {
			warnedOldBackend = true
			console.warn('[announcements] 后端缺少新增诊断字段，可能仍运行旧构建；仅前端热更新不会更新 Rust 后端。')
		}
		if (refreshStartedAt !== null && !next.refreshing && (next.fresh || next.error)) {
			console.info('[announcements] 本次刷新结束', {
				elapsedMs: Math.round(performance.now() - refreshStartedAt), success: next.fresh,
				source: next.source, error: next.error,
			})
			refreshStartedAt = null
		}
	})
	queryQueue = query.catch(() => {})
	return query
}

export function initializeAnnouncements(): Promise<void> {
	if (initialize) return initialize
	console.info('[announcements] 初始化公告：先读取本地缓存，再按 wemgit → GitHub published 获取公告。')
	const token = ++session
	initialize = (async () => {
		const off = await listen('announcements-updated', () => {
			console.info('[announcements] 收到后端公告状态变更通知')
			void reloadAnnouncements().catch((error) => console.warn('[announcements] 无法读取公告状态:', error))
		})
		if (token !== session) { off(); return }
		unlisten = off
		await reloadAnnouncements()
		console.info('[announcements] 开始请求远端公告（后台刷新，不阻塞启动）')
		refreshStartedAt = performance.now()
		const started = await announcementRefresh()
		console.info('[announcements] 后端刷新任务已调度', { started })
		await reloadAnnouncements()
	})().catch((error) => {
		snapshot.value.error = toError(error).message
		snapshot.value.fresh = false
		refreshStartedAt = null
		console.warn('[announcements] 公告初始化或请求失败:', toError(error).message)
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
	console.info('[announcements] 请求公告正文', { id, force })
	const request = announcementGet(id).then((response) => {
		console.info('[announcements] 正文请求结果', {
			id, state: response.announcement.state, hasBody: Boolean(response.markdown),
			cached: response.cached, stale: response.stale, error: response.error,
		})
		if (epoch === bodyEpoch) bodies.set(id, { loading: false, response, error: response.error })
		return response
	}).catch((error) => {
		console.warn('[announcements] 正文请求失败', { id, error: toError(error).message })
		if (epoch === bodyEpoch) bodies.set(id, { loading: false, response: null, error: toError(error).message })
		throw error
	}).finally(() => pendingBodies.delete(key))
	pendingBodies.set(key, request)
	return request
}

export async function suppressAnnouncement(id: string): Promise<void> {
	console.info('[announcements] 保存“不再显示”', { id })
	await announcementSuppress(id)
	console.info('[announcements] “不再显示”已保存', { id })
	for (const item of snapshot.value.active) if (item.id === id) item.suppressed = true
	const body = bodies.get(id)?.response
	if (body) body.announcement.suppressed = true
	snapshot.value.candidate_id = snapshot.value.active.find((item) => !item.suppressed && isAnnouncementActive(item))?.id ?? null
	void reloadAnnouncements().catch((error) => console.warn('Announcement preference refresh failed:', error))
}

export function requestAnnouncements(): void {
	console.info('[announcements] 用户手动打开公告')
	openRequest.value++
}

export function useAnnouncements() {
	const active = computed(() => snapshot.value.active.filter((item) => isAnnouncementActive(item, now.value)))
	return { snapshot, active, now, openRequest, bodies }
}
