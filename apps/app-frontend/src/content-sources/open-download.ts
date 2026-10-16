/**
 * Installing content a source can only offer as a *page*.
 *
 * Most sources hand the launcher a file URL and it fetches the file itself. A
 * source that declares `open_download` instead names a page — a site download
 * behind a captcha, say — and the user completes the download in a window the
 * launcher opens. The launcher captures the resulting file into its staging
 * directory and installs it from there.
 *
 * This is the one place that sequence lives: three call sites (the project page,
 * the browse list and the installed-content modal) install content, and all of
 * them have to fall back to it the same way when a source has no direct URL.
 */

import { installSourceContentFromPath, openSourceDownloadWindow } from './ipc'
import type { InstallMetadata } from './ipc'
import type { ContentSourceProvider, ContentType } from './types'

/** Whether a source can install at all, by either route. */
export function canInstall(provider: ContentSourceProvider | undefined): boolean {
	return Boolean(provider?.resolveDownload || provider?.openDownload)
}

export interface OpenDownloadOptions {
	provider: ContentSourceProvider
	sourceId: string
	projectId: string
	contentType: ContentType
	versionId?: string
	fileId?: string
	instanceId?: string
	meta?: InstallMetadata
}

/**
 * Run the window → stage → install sequence for a source that offers
 * `openDownload`. Throws when the user closes the window without downloading,
 * so the caller's error handling reports it rather than silently doing nothing.
 */
export async function installViaOpenDownload({
	provider,
	sourceId,
	projectId,
	contentType,
	versionId,
	fileId,
	instanceId,
	meta,
}: OpenDownloadOptions) {
	const openDownload = provider.openDownload
	if (!openDownload) {
		throw new Error('该内容源不支持开窗下载。')
	}

	const page = await openDownload.call(provider, projectId, contentType, versionId, fileId)
	const stagedPath = await openSourceDownloadWindow(sourceId, page.url, page.fileName)
	if (!stagedPath) {
		throw new Error('已取消下载。')
	}

	return installSourceContentFromPath(instanceId, contentType, stagedPath, meta)
}
