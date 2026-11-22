import type { ContentUpdaterVersion } from '@modrinth/ui'
import { defineMessages, versionMatchesCompatibilityTarget } from '@modrinth/ui'

import { sourceUpdateSupport } from './installed-content'
import type { SourceContentRecord } from './ipc'
import { sourceProvider } from './registry'
import type { ContentType, ContentVersion } from './types'

export const manualVersionMessages = defineMessages({
	noVersions: { id: 'app.source-content.no-versions', defaultMessage: 'This source does not provide a version list.' },
	unloaded: { id: 'app.source-content.unloaded', defaultMessage: 'This content source is not loaded.' },
	missingAnswer: { id: 'app.source-content.missing-answer', defaultMessage: 'The source returned no update result for this project.' },
	emptyVersions: { id: 'app.source-content.empty-versions', defaultMessage: 'The source returned no selectable versions.' },
})

export class ManualVersionError extends Error {
	constructor(readonly reason: keyof typeof manualVersionMessages) {
		super(manualVersionMessages[reason].defaultMessage)
	}
}

export interface ManualVersionResult {
	action: 'update' | 'switch'
	latestVersionId?: string
	versions: ContentVersion[]
}

export function sourcePickerVersion(version: ContentVersion, type: string): ContentUpdaterVersion {
	const loadersMatter = ['mod', 'plugin', 'modpack', 'datapack'].includes(type)
	return {
		id: version.id,
		name: version.name ?? version.versionNumber ?? version.id,
		version_number: version.versionNumber ?? version.name ?? version.id,
		changelog: version.body ?? null,
		date_published: version.publishedAt ?? null,
		version_type: ['release', 'beta', 'alpha'].includes(version.channel ?? '')
			? version.channel as 'release' | 'beta' | 'alpha' : null,
		game_versions: version.gameVersions ?? [],
		loaders: version.loaders ?? [],
		files: version.files?.map((file) => ({ filename: file.name, size: file.size })),
		compatibility_known: Boolean(version.gameVersions?.length && (!loadersMatter || version.loaders?.length)),
	}
}

export async function loadSourceVersions(record: SourceContentRecord): Promise<ContentVersion[]> {
	const provider = sourceProvider(record.source_id)
	if (!provider?.versions) throw new ManualVersionError('noVersions')
	const versions = await provider.versions(record.project_id, {
		contentType: record.content_type as ContentType, sort: 'newest', page: 1, limit: 50,
	})
	const unique = new Map<string, ContentVersion>()
	for (const version of versions) {
		if (version.id && !unique.has(version.id)) unique.set(version.id, version)
	}
	return [...unique.values()].sort((a, b) => {
		const first = Date.parse(a.publishedAt ?? ''), second = Date.parse(b.publishedAt ?? '')
		return Number.isFinite(first) && Number.isFinite(second) ? second - first : 0
	})
}

/** Unknown current identity or compatibility is not proof of a newer usable version. */
export async function inspectSourceVersions(
	record: SourceContentRecord,
	context: { gameVersion?: string; loader?: string },
): Promise<ManualVersionResult> {
	const provider = sourceProvider(record.source_id)
	if (!provider) throw new ManualVersionError('unloaded')
	let explicitlyCurrent = false
	if (sourceUpdateSupport(record).canCheck && provider.checkUpdate) {
		const answers = await provider.checkUpdate([{
			id: record.project_id, contentType: record.content_type as ContentType,
			currentVersionId: record.version_id ?? undefined,
			gameVersion: context.gameVersion, loader: context.loader,
			installRule: (record.install_rule as Record<string, unknown> | null) ?? undefined,
		}])
		const answer = answers.find((item) => item.id === record.project_id)
		if (!answer) throw new ManualVersionError('missingAnswer')
		if (record.version_id && answer.latestVersionId && answer.latestVersionId !== record.version_id) {
			return { action: 'update', latestVersionId: answer.latestVersionId, versions: [] }
		}
		explicitlyCurrent = Boolean(record.version_id)
	}
	const versions = await loadSourceVersions(record)
	if (versions.length === 0) throw new ManualVersionError('emptyVersions')
	const current = versions.find((version) => version.id === record.version_id)
	const currentTime = Date.parse(current?.publishedAt ?? '')
	if (current && !explicitlyCurrent && Number.isFinite(currentTime)) {
		const latest = versions.find((version) => {
			if (version.id === current.id || !Number.isFinite(Date.parse(version.publishedAt ?? ''))
				|| Date.parse(version.publishedAt!) <= currentTime) return false
			const display = sourcePickerVersion(version, record.content_type)
			return display.compatibility_known && versionMatchesCompatibilityTarget(display, {
				gameVersion: context.gameVersion ?? '', loader: context.loader ?? '', projectType: record.content_type,
			})
		})
		if (latest) return { action: 'update', latestVersionId: latest.id, versions }
	}
	return { action: 'switch', versions }
}
