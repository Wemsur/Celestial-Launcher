/**
 * The shared state of a content source's project page: what the source says
 * about the project, and everything needed to install it.
 *
 * The project itself is cached module-level rather than per component: the
 * header, the tabs and the version page all want the same project, and asking
 * the source three times for one page view is three round trips for data that
 * has not changed. A source is fetched live over the network with no cache of
 * its own, so this is the only thing standing between a tab switch and another
 * request.
 */

import { useQueryClient } from '@tanstack/vue-query'
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'
import { useRoute } from 'vue-router'

import { installedManifest, sourceAppearance } from '@/content-sources/catalog'
import { installSourceContent, listInstalledSourceContent } from '@/content-sources/ipc'
import { sourceProvider } from '@/content-sources/registry'
import type {
	ContentFile,
	ContentProject,
	ContentSourceProvider,
	ContentType,
	ContentVersion,
} from '@/content-sources/types'
import { toError } from '@/helpers/errors'
import { injectContentInstall } from '@/providers/content-install'

const STALE_TIME = 120_000

interface LoadedProject {
	project: ContentProject | null
	versions: ContentVersion[]
}

/** Fetched projects, keyed by source, project and type. */
const cache = new Map<string, { at: number; data: LoadedProject }>()
/** Loads in flight, so two components mounting together share one request. */
const inFlight = new Map<string, Promise<LoadedProject>>()

function cacheKey(sourceId: string, projectId: string, type: string): string {
	return `${sourceId}:${projectId}:${type}`
}

async function loadProject(
	provider: ContentSourceProvider,
	sourceId: string,
	projectId: string,
	contentType: ContentType,
): Promise<LoadedProject> {
	const key = cacheKey(sourceId, projectId, contentType)
	const cached = cache.get(key)
	if (cached && Date.now() - cached.at < STALE_TIME) {
		return cached.data
	}
	const pending = inFlight.get(key)
	if (pending) return pending

	const load = (async () => {
		const data: LoadedProject = {
			project: provider.detail
				? await provider.detail(projectId, contentType)
				: null,
			versions: provider.versions
				? await provider.versions(projectId, {
						contentType,
						sort: 'newest',
						page: 1,
						limit: 50,
					})
				: [],
		}
		cache.set(key, { at: Date.now(), data })
		return data
	})()

	inFlight.set(key, load)
	try {
		return await load
	} finally {
		inFlight.delete(key)
	}
}

/** Forget a project, so the next read goes back to the source. */
export function forgetSourceProject(sourceId: string, projectId: string): void {
	for (const key of [...cache.keys()]) {
		if (key.startsWith(`${sourceId}:${projectId}:`)) cache.delete(key)
	}
}

export interface SourceProjectState {
	sourceId: ComputedRef<string>
	projectId: ComputedRef<string>
	contentType: ComputedRef<ContentType>
	provider: ComputedRef<ContentSourceProvider | undefined>
	appearance: ComputedRef<ReturnType<typeof sourceAppearance>>
	project: Ref<ContentProject | null>
	versions: Ref<ContentVersion[]>
	loading: Ref<boolean>
	error: Ref<string | null>
	/** The instance this browse session installs into, when there is one. */
	instanceId: ComputedRef<string | undefined>
	/** Whether this project is already installed in that instance. */
	installed: Ref<boolean>
	installing: Ref<boolean>
	/** Whether a version offers a choice of files worth showing. */
	filesOf: (version: ContentVersion | undefined) => ContentFile[]
	install: (options?: {
		versionId?: string
		versionName?: string
		fileId?: string
	}) => Promise<void>
}

export function useSourceProject(): SourceProjectState {
	const route = useRoute()
	const queryClient = useQueryClient()
	const { showSourceInstallModal, showSourceModpackInstall } = injectContentInstall()

	const sourceId = computed(() => String(route.params.sourceId ?? ''))
	const projectId = computed(() => String(route.params.projectId ?? ''))
	const provider = computed(() => sourceProvider(sourceId.value))

	// The split route carries the type in the path; the full-width route in the
	// query. Falling back to the source's own type keeps a hand-typed URL usable.
	const contentType = computed(
		() =>
			String(
				route.params.projectType ??
					route.query.type ??
					installedManifest(sourceId.value)?.content_types[0] ??
					'mod',
			) as ContentType,
	)

	const appearance = computed(() => sourceAppearance(sourceId.value))

	const instanceId = computed(() =>
		route.query.i ? String(route.query.i) : undefined,
	)

	const project = ref<ContentProject | null>(null)
	const versions = ref<ContentVersion[]>([])
	const loading = ref(true)
	const error = ref<string | null>(null)
	const installed = ref(false)
	const installing = ref(false)

	/**
	 * Whether the project is already installed here. Only knowable with an
	 * instance in context: the record lives with the instance, and a skin — which
	 * has no instance — has none to look up.
	 */
	async function refreshInstalled(): Promise<void> {
		const target = instanceId.value
		if (!target || !provider.value) {
			installed.value = false
			return
		}
		try {
			const records = await listInstalledSourceContent(target)
			installed.value = records.some(
				(record) =>
					record.source_id === sourceId.value &&
					record.project_id === projectId.value,
			)
		} catch {
			// Not knowing is the same as not being installed, as far as the page is
			// concerned; the install itself will report any real failure.
			installed.value = false
		}
	}

	async function load(): Promise<void> {
		const current = provider.value
		if (!current) {
			error.value = '内容源未加载。'
			loading.value = false
			project.value = null
			versions.value = []
			return
		}
		loading.value = true
		error.value = null
		try {
			const data = await loadProject(
				current,
				sourceId.value,
				projectId.value,
				contentType.value,
			)
			project.value = data.project
			versions.value = data.versions
			await refreshInstalled()
		} catch (caught) {
			error.value = toError(caught).message
		} finally {
			loading.value = false
		}
	}

	watch(
		() => [sourceId.value, projectId.value, contentType.value, instanceId.value],
		() => void load(),
		{ immediate: true },
	)

	function filesOf(version: ContentVersion | undefined): ContentFile[] {
		// One file is not a choice; the source's own rule should decide it.
		const files = version?.files ?? []
		return files.length > 1 ? files : []
	}

	/**
	 * Install this project, or one of its versions.
	 *
	 * With no instance in context the launcher first asks which one, through the
	 * same picker a Modrinth project uses — a source does not know what an
	 * instance is, so the choice stays the launcher's either way.
	 */
	async function install(options?: {
		versionId?: string
		versionName?: string
		fileId?: string
	}): Promise<void> {
		const current = provider.value
		if (!current?.resolveDownload) {
			throw new Error('该内容源不支持直接下载。')
		}

		const run = async (targetInstanceId?: string) => {
			installing.value = true
			try {
				const descriptor = await current.resolveDownload(
					projectId.value,
					options?.versionId,
					options?.fileId,
				)
				const result = await installSourceContent(
					targetInstanceId,
					contentType.value,
					descriptor,
					{
						sourceId: sourceId.value,
						projectId: projectId.value,
						versionId: options?.versionId,
						versionName: options?.versionName,
						name: project.value?.name ?? projectId.value,
						iconUrl: project.value?.iconUrl ?? undefined,
						// A hand-picked file is remembered as the rule, so a later
						// update check matches the same asset rather than re-guessing.
						installRule: options?.fileId ? { fileId: options.fileId } : undefined,
					},
				)
				void queryClient.invalidateQueries({ queryKey: ['instance'] })
				forgetSourceProject(sourceId.value, projectId.value)
				await refreshInstalled()
				return result
			} finally {
				installing.value = false
			}
		}

	// A modpack *is* an instance: it runs the pack flow instead of the picker.
	// `installing` is held across the whole flow — the library picker and the
	// staging step take real time, and without it the install button stays
	// enabled and can be clicked again mid-flow.
	if (contentType.value === 'modpack') {
		installing.value = true
		try {
			const installed = await showSourceModpackInstall({
				sourceId: sourceId.value,
				projectId: projectId.value,
				versionId: options?.versionId,
				title: project.value?.name ?? projectId.value,
				iconUrl: project.value?.iconUrl ?? undefined,
			})
			// A cancelled picker installs nothing, so the project stays unknown
			// to the page and must not be forgotten.
			if (installed) {
				forgetSourceProject(sourceId.value, projectId.value)
				await refreshInstalled()
			}
		} finally {
			installing.value = false
		}
		return
	}

	// A skin belongs to the launcher; everything else needs an instance.
	if (contentType.value === 'skin') {
		await run(undefined)
		return
	}
	if (instanceId.value) {
		await run(instanceId.value)
		return
	}
	// Ask which instance first. The install then runs from the picker's own
	// callback rather than from a promise wrapped around it — a picker the user
	// cancels never calls back, and a promise waiting on that would never
	// settle. The picker reports any failure itself.
	await showSourceInstallModal((chosen) => run(chosen), {
		title: project.value?.name ?? projectId.value,
		iconUrl: project.value?.iconUrl ?? undefined,
		sourceId: sourceId.value,
		projectId: projectId.value,
		projectType: String(contentType.value),
		versions: versions.value,
	})
}

	return {
		sourceId,
		projectId,
		contentType,
		provider,
		appearance,
		project,
		versions,
		loading,
		error,
		instanceId,
		installed,
		installing,
		filesOf,
		install,
	}
}
