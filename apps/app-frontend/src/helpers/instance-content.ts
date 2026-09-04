import type { ContentItem, ManagedContentProject, ManagedContentVersion } from '@modrinth/ui'

import {
	get_content_items,
	get_content_skeleton,
	get_linked_modpack_info,
	type LinkedModpackInfo,
} from '@/helpers/instance'
import type { CacheBehaviour } from '@/helpers/types'

export type InstanceContentData = {
	path: string
	contentItems: ContentItem[] | null
	modpack: InstanceContentModpackData | null
	/**
	 * True while these rows still carry only local information. The list is safe
	 * to render, but titles, icons and authors are still on their way.
	 */
	partial?: boolean
}

export type InstanceContentModpackData = {
	project: ManagedContentProject
	version: ManagedContentVersion | null
	updateVersionId: string | null
}

/**
 * The cheapest content read there is — no SQLite, no network, no directory walk.
 *
 * Used as placeholder data so the content tab paints rows immediately instead of
 * sitting empty while {@link loadInstanceContentData} resolves metadata. Errors
 * are swallowed: a placeholder that fails to load is not worth surfacing, the
 * real query reports its own failures.
 */
export async function loadInstanceContentSkeleton(
	path: string,
): Promise<InstanceContentData | null> {
	try {
		const contentItems = await get_content_skeleton(path)
		return { path, contentItems, modpack: null, partial: true }
	} catch (error) {
		// Not surfaced to the user — the real query reports its own failures — but
		// not silent either: a rejected invoke here (a missing Tauri permission, say)
		// would otherwise look exactly like "the placeholder just never helps".
		console.debug('[content] skeleton placeholder unavailable', error)
		return null
	}
}

export async function loadInstanceContentData(
	path: string,
	cacheBehaviour?: CacheBehaviour,
	onError?: (error: Error) => unknown,
): Promise<InstanceContentData> {
	const [contentItems, modpackInfo] = await Promise.all([
		get_content_items(path, cacheBehaviour).catch((error) => handleLoadError(error, onError)),
		get_linked_modpack_info(path, cacheBehaviour).catch((error) => handleLoadError(error, onError)),
	])

	return {
		path,
		contentItems: (contentItems as ContentItem[] | null | undefined) ?? null,
		modpack: normalizeLinkedModpackInfo(modpackInfo as LinkedModpackInfo | null | undefined),
	}
}

function handleLoadError(error: unknown, onError?: (error: Error) => unknown) {
	if (!onError) throw error
	onError(error as Error)
	return null
}

function normalizeLinkedModpackInfo(
	modpackInfo: LinkedModpackInfo | null | undefined,
): InstanceContentModpackData | null {
	if (!modpackInfo) return null

	return {
		project: {
			...modpackInfo.project,
			slug: modpackInfo.project.slug ?? modpackInfo.project.id,
			icon_url: modpackInfo.project.icon_url ?? undefined,
		},
		version: modpackInfo.version
			? {
					...modpackInfo.version,
					date_published: modpackInfo.version.date_published.toString(),
				}
			: null,
		updateVersionId: modpackInfo.update_version_id,
	}
}
