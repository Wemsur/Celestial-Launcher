/**
 * Source-installed content as the instance's content tab sees it.
 *
 * The launcher's own content list knows nothing about content sources: a file a
 * source placed is just a file on disk, with no Modrinth project behind it, so
 * its row has no author, no version and no update badge. The source's install
 * records hold exactly what the row is missing — which source, which project,
 * which version — and `relative_path` is the one key both sides share.
 *
 * This module joins the two and decides what an update check may ask about, so
 * the page and the source-content modal cannot drift apart on the rules.
 */

import { sourceAppearance } from './catalog'
import type { SourceContentRecord } from './ipc'
import { contentSourceSummaries, sourceProvider } from './registry'
import type { ContentType, UpdateQueryItem } from './types'

/** Install records keyed by the path the content list reports for the file. */
export type SourceRecordsByPath = Map<string, SourceContentRecord>

/**
 * Index records by path, so a content row can find the record for its file.
 *
 * Both spellings of a path are registered: the instance's own list reports a
 * disabled file as `mods/x.jar.disabled` (that is its name on disk), while a
 * record may still carry the enabled name. Indexing both means a toggle cannot
 * make the badge disappear.
 */
export function indexSourceRecords(records: SourceContentRecord[]): SourceRecordsByPath {
	const byPath: SourceRecordsByPath = new Map()
	for (const record of records) {
		byPath.set(record.relative_path, record)
		byPath.set(record.relative_path.replace(/\.disabled$/, ''), record)
	}
	return byPath
}

/** The record for a content row, matched on the path both sides report. */
export function sourceRecordFor(
	byPath: SourceRecordsByPath,
	filePath: string | undefined,
): SourceContentRecord | undefined {
	if (!filePath) return undefined
	return byPath.get(filePath) ?? byPath.get(filePath.replace(/\.disabled$/, ''))
}

/**
 * Whether a record's content can take part in the launcher's update flow at all.
 *
 * A world is installed as a fresh copy every time and never replaced, so it has
 * no "newer version" to apply; a skin belongs to the launcher and is not tracked.
 */
export function canTrackUpdates(record: SourceContentRecord): boolean {
	return record.content_type !== 'world' && record.content_type !== 'skin'
}

/** A source's manifest, when the source is installed and enabled. */
export function sourceManifest(sourceId: string) {
	return (
		contentSourceSummaries.value.find((summary) => summary.id === sourceId)?.manifest ?? null
	)
}

/**
 * Whether the launcher may offer an update for this record.
 *
 * The manifest already says what a source supports: `update_check` means it can
 * answer "is there something newer", and `resolve_download` means the answer can
 * be applied without the user fetching the file themselves. A source that can
 * only `open_download` can still be updated, but only one item at a time, in a
 * window the user completes — never as part of a silent bulk update.
 */
export function sourceUpdateSupport(record: SourceContentRecord): {
	canCheck: boolean
	canApplyAutomatically: boolean
} {
	if (!canTrackUpdates(record)) {
		return { canCheck: false, canApplyAutomatically: false }
	}
	const manifest = sourceManifest(record.source_id)
	const provider = sourceProvider(record.source_id)
	// The manifest is what a source *claims*; the loaded provider is what it can
	// actually do. Both have to agree, or the launcher would offer an update
	// whose check function does not exist.
	const canCheck = Boolean(manifest?.capabilities.update_check && provider?.checkUpdate)
	const canApplyAutomatically = Boolean(
		canCheck && manifest?.capabilities.resolve_download,
	)
	return { canCheck, canApplyAutomatically }
}

/**
 * One record's update answer. An error is kept per record rather than per
 * source so the source-content modal can say which check failed; the instance
 * content list only reads the successful answers.
 */
export type SourceUpdateAnswer =
	| { status: 'ok'; latestVersionId?: string; latestVersionName?: string }
	| { status: 'error'; message: string }

/** Answers keyed by install-record id. */
export type SourceUpdateAnswers = Record<string, SourceUpdateAnswer>

/**
 * Ask every source that has content installed here whether anything is newer.
 *
 * Each source is asked once, with all of its records, and its answers land as
 * soon as it replies — a slow source does not hold up the ones already done.
 * A source that cannot check contributes no answers; one that throws contributes
 * an error for each of its records rather than failing the whole check.
 */
export async function checkSourceUpdates(
	records: SourceContentRecord[],
	context: { gameVersion?: string; loader?: string },
): Promise<SourceUpdateAnswers> {
	const bySource = new Map<string, SourceContentRecord[]>()
	for (const record of records) {
		if (!sourceUpdateSupport(record).canCheck) continue
		const list = bySource.get(record.source_id)
		if (list) {
			list.push(record)
		} else {
			bySource.set(record.source_id, [record])
		}
	}

	const answers: SourceUpdateAnswers = {}

	await Promise.all(
		[...bySource.entries()].map(async ([sourceId, items]) => {
			const provider = sourceProvider(sourceId)
			if (!provider?.checkUpdate) return
			try {
				const query: UpdateQueryItem[] = items.map((item) => ({
					id: item.project_id,
					contentType: item.content_type as ContentType,
					currentVersionId: item.version_id ?? undefined,
					gameVersion: context.gameVersion,
					loader: context.loader,
					installRule: (item.install_rule as Record<string, unknown> | null) ?? undefined,
				}))
				const results = await provider.checkUpdate(query)
				const byProject = new Map(results.map((result) => [result.id, result]))
				for (const item of items) {
					const result = byProject.get(item.project_id)
					answers[item.id] = {
						status: 'ok',
						latestVersionId: result?.latestVersionId,
						latestVersionName: result?.latestVersionName,
					}
				}
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error)
				for (const item of items) {
					answers[item.id] = { status: 'error', message }
				}
			}
		}),
	)

	return answers
}

/** The newer version a source reported for a record, if there is one. */
export function latestSourceVersion(
	answers: SourceUpdateAnswers,
	record: SourceContentRecord,
): string | null {
	const answer = answers[record.id]
	return answer?.status === 'ok' ? (answer.latestVersionId ?? null) : null
}

/** The badge a row shows for the source it came from. */
export function sourceOriginOf(record: SourceContentRecord) {
	const appearance = sourceAppearance(record.source_id)
	return {
		sourceId: record.source_id,
		name: appearance.name,
		iconUrl: appearance.iconUrl,
		link: `/source/${encodeURIComponent(record.source_id)}/${encodeURIComponent(record.project_id)}`,
	}
}
