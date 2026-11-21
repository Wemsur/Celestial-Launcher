import { invoke } from '@tauri-apps/api/core'

export interface AnnouncementMetadata {
	id: string
	issue_number: number | null
	title: string | null
	level: 'info' | 'warning' | 'critical' | null
	published_at: string
	updated_at: string
	starts_at: string | null
	expires_at: string | null
	state: 'published' | 'ended' | 'withdrawn'
	body_path: string | null
	body_sha256: string | null
	suppressed: boolean
	body_cached: boolean
}

export interface AnnouncementSnapshot {
	generation: number | null
	source: string | null
	last_success_at: string | null
	refreshing: boolean
	fresh: boolean
	error: string | null
	active: AnnouncementMetadata[]
	total_count: number
	visible_count: number
	candidate_id: string | null
}

export interface AnnouncementPage {
	items: AnnouncementMetadata[]
	next_cursor: number | null
}

export interface AnnouncementBody {
	announcement: AnnouncementMetadata
	markdown: string | null
	cached: boolean
	stale: boolean
	error: string | null
}

export const announcementSnapshot = () => invoke<AnnouncementSnapshot>('plugin:announcements|announcement_snapshot')
export const announcementList = (cursor: number | null, limit = 10) =>
	invoke<AnnouncementPage>('plugin:announcements|announcement_list', { cursor, limit })
export const announcementGet = (id: string) => invoke<AnnouncementBody>('plugin:announcements|announcement_get', { id })
export const announcementRefresh = () => invoke<boolean>('plugin:announcements|announcement_refresh')
export const announcementSuppress = (id: string) => invoke<void>('plugin:announcements|announcement_suppress', { id })
