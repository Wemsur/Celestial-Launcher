import type {
	AbstractWebNotificationManager,
	CreationFlowContextValue,
	CreationFlowModal,
} from '@modrinth/ui'
import { provide, ref, useTemplateRef } from 'vue'
import type { ComponentExposed } from 'vue-component-type-helpers'
import { useRouter } from 'vue-router'

import type UnknownPackWarningModal from '@/components/ui/install_flow/UnknownPackWarningModal.vue'
import type ModpackAlreadyInstalledModal from '@/components/ui/modal/ModpackAlreadyInstalledModal.vue'
import { useAppEvent } from '@/composables/use-app-event'
import { useAppSettings } from '@/composables/use-app-settings.ts'
import type { AppEvents } from '../app-events'
import { trackEvent } from '@/helpers/analytics'
import { get_search_results } from '@/helpers/cache.js'
import { import_instance } from '@/helpers/import.js'
import {
	type CreatePackLocation,
	install_create_instance,
	install_create_modpack_instance,
	install_get_modpack_preview,
	installJobInstanceId,
} from '@/helpers/install'
import { list } from '@/helpers/instance'
import { library_default_path, library_list } from '@/helpers/library'
import { pickInstallLibrary } from '@/providers/library-picker'
import { get_loader_versions as getLoaderManifest } from '@/helpers/metadata.js'
import type { InstanceIconConfig, InstanceLoader } from '@/helpers/types'

export function setupCreationModal(
	notificationManager: AbstractWebNotificationManager,
	getGeneratedIconConfig?: (iconPath: string) => InstanceIconConfig | null,
	appEvents?: AppEvents,
) {
	const { handleError } = notificationManager
	const router = useRouter()
	const appSettings = useAppSettings()

	const installationModal =
		useTemplateRef<ComponentExposed<typeof CreationFlowModal>>('installationModal')
	const unknownPackWarningModal =
		useTemplateRef<InstanceType<typeof UnknownPackWarningModal>>('unknownPackWarningModal')
	const modpackAlreadyInstalledModal = ref<InstanceType<typeof ModpackAlreadyInstalledModal>>()

	// Load available libraries for the creation modal
	const availableLibraries = ref<Array<{ path: string; name: string }>>([])
	const defaultLibraryPath = ref<string | null>(null)
	// Map of library path → format string ('modrinth' | 'minecraft')
	const libraryFormatMap = ref<Map<string, string>>(new Map())

	async function refreshLibraries() {
		try {
			const config = await library_list()
			availableLibraries.value = config.libraries
			defaultLibraryPath.value = await library_default_path().catch(() => null)
			const fmtMap = new Map<string, string>()
			for (const lib of config.libraries) {
				fmtMap.set(lib.path, lib.type.toLowerCase())
			}
			libraryFormatMap.value = fmtMap
		} catch {
			// ignore
		}
	}
	const librariesReady = refreshLibraries().catch(() => {})
	useAppEvent('library_changed', refreshLibraries, appEvents)

	// ── showCreationModal with preselection ────────────────────────────────
	// Read the currently active library from localStorage so the modal
	// always pre-selects the right library regardless of which page opens it.
	// The home page removes that key while the "全部实例" tab is active, in which
	// case we fall back to the default library instead of leaving the picker
	// empty.
	function readActiveLibraryPath(): string | null {
		if (typeof window === 'undefined') return null
		return localStorage.getItem('celestial-library-active-tab') ?? null
	}

	function resolvePreselectedLibraryPath(): string | null {
		const active = readActiveLibraryPath()
		if (active && active !== 'all') return active
		// The picker's option values are library paths, and the default library's
		// own entry can be `<default>/profiles` while `library_default_path()`
		// returns `<default>`, so match it the same way the label does. Only an
		// entry that is really in the list can be preselected — a bare path the
		// picker does not offer would show as an empty box.
		const fallback = defaultLibraryPath.value
		if (!fallback) return null
		const normDefault = fallback.replace(/\\/g, '/').toLowerCase().replace(/\/+$/, '')
		const match = availableLibraries.value.find((lib) => {
			const normLib = lib.path.replace(/\\/g, '/').toLowerCase().replace(/\/+$/, '')
			return normLib === normDefault || normLib === `${normDefault}/profiles`
		})
		return match?.path ?? null
	}

	const preselectedLibraryPath = ref<string | null>(resolvePreselectedLibraryPath())
	const lastSelectedLibraryPath = ref<string | null>(preselectedLibraryPath.value)

	provide('showCreationModal', async () => {
		// Opening the modal in the first moments after launch must wait for the
		// library list: with an empty list the picker hides itself, and the create
		// button would then not be gated on a library at all.
		await librariesReady
		// Re-read persisted tab each time so it stays in sync
		const currentTab = resolvePreselectedLibraryPath()
		lastSelectedLibraryPath.value = currentTab
		preselectedLibraryPath.value = currentTab
		installationModal.value?.show(currentTab)
	})

	function setModpackAlreadyInstalledModal(
		modal: InstanceType<typeof ModpackAlreadyInstalledModal>,
	) {
		modpackAlreadyInstalledModal.value = modal
	}

	async function fetchExistingInstanceNames(): Promise<string[]> {
		const instances = (await list().catch(handleError)) ?? []
		return instances?.map((i) => i.name) ?? []
	}

	provide('showImportModal', async () => {
		await installationModal.value?.show()
		installationModal.value?.ctx.setImportMode()
	})

	async function navigateToCreatedInstance(
		job: Awaited<ReturnType<typeof install_create_instance>>,
	) {
		const instanceId = installJobInstanceId(job)
		if (instanceId) {
			await router.push(`/instance/${encodeURIComponent(instanceId)}`)
		}
	}

	async function proceedWithModpackCreation(
		projectId: string,
		versionId: string,
		name: string,
		iconUrl?: string,
	) {
		const job = await install_create_modpack_instance({
			type: 'fromVersionId',
			project_id: projectId,
			version_id: versionId,
			title: name,
			icon_url: iconUrl,
		})
		await navigateToCreatedInstance(job)
		trackEvent('InstanceCreate', { source: 'CreationModalModpack' })
	}

	async function proceedWithModpackFileCreation(location: CreatePackLocation) {
		const { cancelled, library } = await pickInstallLibrary()
		if (cancelled) return
		const job = await install_create_modpack_instance(
			location,
			null,
			library?.path ?? null,
			library?.format ?? null,
		)
		await navigateToCreatedInstance(job)
		trackEvent('InstanceCreate', { source: 'CreationModalModpackFile' })
	}

	async function handleCreate(config: CreationFlowContextValue) {
		try {
			if (config.modpackSelection.value) {
				const { projectId, versionId, name, iconUrl } = config.modpackSelection.value

				const instances = (await list().catch(handleError)) ?? []
				const existingInstance = instances?.find((i) => i.link?.project_id === projectId)

				if (existingInstance && !appSettings.getFeatureFlag('skip_non_essential_warnings')) {
					pendingModpackCreation.value = { projectId, versionId, name, iconUrl }
					installationModal.value?.hide()
					modpackAlreadyInstalledModal.value?.show(existingInstance.name, existingInstance.id)
					return
				}
			}

			installationModal.value?.hide()

			if (config.isImportMode.value) {
				let importCount = 0
				let importedInstanceId: string | null = null
				for (const [launcherName, instanceSet] of Object.entries(
					config.importSelectedInstances.value,
				)) {
					const launcher = config.importLaunchers.value.find((l) => l.name === launcherName)
					if (!launcher || instanceSet.size === 0) continue
					for (const name of instanceSet) {
						importCount += 1
						const job = await import_instance(launcher.name, launcher.path, name).catch(handleError)
						if (job) {
							importedInstanceId = installJobInstanceId(job)
						}
					}
				}
				trackEvent('InstanceCreate', { source: 'CreationModalImport' })
				if (importCount === 1 && importedInstanceId) {
					await router.push(`/instance/${encodeURIComponent(importedInstanceId)}`)
				}
				return
			}

			if (config.modpackSelection.value) {
				const { projectId, versionId, name, iconUrl } = config.modpackSelection.value
				await proceedWithModpackCreation(projectId, versionId, name, iconUrl)
				return
			}

			if (config.modpackFilePath.value) {
				const location: CreatePackLocation = {
					type: 'fromFile',
					path: config.modpackFilePath.value,
				}
				const preview = await install_get_modpack_preview(location)

				if (preview.unknownFile || preview.externalFilesInModpack.length > 0) {
					const splitPath = config.modpackFilePath.value.split(/[\\/]/)
					const fileName = splitPath
						? splitPath[splitPath.length - 1]
						: config.modpackFilePath.value
					if (unknownPackWarningModal.value) {
						unknownPackWarningModal.value?.show(
							() => proceedWithModpackFileCreation(location).catch(handleError),
							fileName,
							preview.externalFilesInModpack,
						)
					} else {
						await proceedWithModpackFileCreation(location)
					}
				} else {
					await proceedWithModpackFileCreation(location)
				}
				return
			}

			// Custom/vanilla setup
			const loader = config.hideLoaderChips.value
				? 'vanilla'
				: (config.selectedLoader.value ?? 'vanilla')
			const loaderVersion = config.hideLoaderVersion.value
				? null
				: (config.selectedLoaderVersion.value ?? config.loaderVersionType.value)
			const iconPath = config.instanceIconPath.value ?? null
			const name = config.instanceName.value.trim() || config.autoInstanceName.value

			const job = await install_create_instance({
				name,
				gameVersion: config.selectedGameVersion.value!,
				loader: loader as InstanceLoader,
				loaderVersion,
				iconPath,
				iconConfig: iconPath ? getGeneratedIconConfig?.(iconPath) : null,
                libraryPath: config.selectedLibraryPath.value,
                instanceFormat: config.selectedLibraryPath.value
					? libraryFormatMap.value.get(config.selectedLibraryPath.value) ?? null
					: null,
			})
			await navigateToCreatedInstance(job)

			trackEvent('InstanceCreate', {
				source: 'CreationModal',
			})
		} catch (err) {
			handleError(err as Error)
		}
	}

	const pendingModpackCreation = ref<{
		projectId: string
		versionId: string
		name: string
		iconUrl?: string
	} | null>(null)

	async function handleModpackDuplicateCreateAnyway() {
		if (!pendingModpackCreation.value) return
		const { projectId, versionId, name, iconUrl } = pendingModpackCreation.value
		pendingModpackCreation.value = null
		try {
			await proceedWithModpackCreation(projectId, versionId, name, iconUrl)
		} catch (error) {
			handleError(error as Error)
		}
	}

	function handleModpackDuplicateGoToInstance(instanceId: string) {
		pendingModpackCreation.value = null
		router.push(`/instance/${encodeURIComponent(instanceId)}/`)
	}

	function handleBrowseModpacks() {
		installationModal.value?.hide()
		router.push('/browse/modpack')
	}

	async function searchProjects(query: string, limit: number = 10) {
		const projectTypes = ['mod', 'modpack', 'resourcepack', 'shader', 'datapack']
		const facets = JSON.stringify([projectTypes.map((type) => `project_type:${type}`)])
		const params = [`facets=${encodeURIComponent(facets)}`, `limit=${limit}`]
		if (query) {
			params.push(`query=${encodeURIComponent(query)}`)
		}
		const raw = await get_search_results(`?${params.join('&')}`)
		if (raw?.result) return raw.result
		return { hits: [], offset: 0, limit, total_hits: 0 }
	}

	return {
		installationModal,
		unknownPackWarningModal,
		fetchExistingInstanceNames,
		handleCreate,
		handleBrowseModpacks,
		searchProjects,
		getLoaderManifest,
		setModpackAlreadyInstalledModal,
		handleModpackDuplicateCreateAnyway,
		handleModpackDuplicateGoToInstance,
		availableLibraries,
		defaultLibraryPath,
		preselectedLibraryPath,
	}
}
