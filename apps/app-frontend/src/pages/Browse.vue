<script setup lang="ts">
import type { Labrinth } from '@modrinth/api-client'
import {
    CheckIcon,
    CompassIcon,
    ExternalIcon,
    GlobeIcon,
    PlusIcon,
    ServerStackIcon,
    SpinnerIcon,
} from '@modrinth/assets'
import type { BrowseInstallContentType, CardAction, FilterType, ProjectType, Tags } from '@modrinth/ui'
import {
    BrowsePageLayout,
    BrowseSidebar,
    commonMessages,
    ContextMenu,
    CreationFlowModal,
    defineMessages,
    formatProjectTypeSentence,
    getLatestMatchingInstallVersion,
    getSelectedInstallPreferences,
    getTargetInstallPreferences,
    getCategoryHeaderMessage,
    injectNotificationManager,
    LoadingIndicator,
    preferencesDiffer,
    provideBrowseManager,
    requestInstall,
    resolveInstallPlan,
    stripServerRuntimeInstallFilters,
    stripServerRuntimeInstallOverrides,
    useBrowseSearch,
    useDebugLogger,
    useVIntl,
} from '@modrinth/ui'
import { useQuery, useQueryClient } from '@tanstack/vue-query'
import type { Ref } from 'vue'
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import type { LocationQuery } from 'vue-router'
import { RouterView, useRoute, useRouter } from 'vue-router'

import i18n from '@/i18n.config'
import { useAppServerBrowse } from '@/composables/browse/use-app-server-browse'
import { useAppEvent } from '@/composables/use-app-event'
import { useAppSettings } from '@/composables/use-app-settings.ts'
import { useContentTranslation } from '@/composables/use-content-translation.ts'
import {
    browseListLocation,
    browsePathFor,
    openProjectsInSplitView,
    splitDetailTintColor,
    useSplitView,
} from '@/composables/use-split-view.ts'
import {
    fromSearchResult,
    modrinthHitOf,
    type SourcedSearchResult,
    toSearchResult,
} from '@/content-sources/adapter'
import { type ActiveSourceQuery, searchAcrossSources } from '@/content-sources/aggregate'
import {
    flattenSourceGroups,
    type SidebarGroup,
    sourceFiltersFromSelection,
    useSourceFilters,
} from '@/content-sources/filters'
import {
    MODRINTH_CONTENT_TYPES,
    type SelectableSource,
    selectableSources,
    sourceAppearance,
    sourceSupportsType,
    sourceTintStyle,
} from '@/content-sources/catalog'
import { installSourceContent } from '@/content-sources/ipc'
import {
    isSourceActive,
    MODRINTH_SOURCE_ID,
    sourceMultiplier,
    sourcePreferences,
} from '@/content-sources/preferences'
import { sourceProvider } from '@/content-sources/registry'
import SourceSelector from '@/content-sources/SourceSelector.vue'
import type { ContentQuery, ContentSort, ContentType } from '@/content-sources/types'
import { get_project, get_search_results_v3, get_version_many } from '@/helpers/cache.js'
import { toError } from '@/helpers/errors'
import {
    get_installed_project_ids as getInstalledProjectIds,
    getInstanceIconUrl,
    list as listInstances,
} from '@/helpers/instance'
import { get_loader_versions as getLoaderManifest } from '@/helpers/metadata'
import { get as getSettings, set as setSettings } from '@/helpers/settings.ts'
import { get_categories, get_game_versions, get_loaders } from '@/helpers/tags'
import { get_instance_worlds } from '@/helpers/worlds'
import {
    instanceDetailQueryOptions,
    instanceKeys,
    instanceLinkedProjectQueryOptions,
} from '@/pages/instance/query-options'
import { type BreadcrumbDefinition, injectBreadcrumbManager } from '@/providers/breadcrumbs'
import { injectContentInstall } from '@/providers/content-install'
import { injectServerInstall } from '@/providers/server-install'
import {
    createServerInstallContent,
    provideServerInstallContent,
} from '@/providers/setup/server-install-content'

const { handleError, addNotification } = injectNotificationManager()
const { formatMessage } = useVIntl()
const { installingServerProjects, playServerProject, showAddServerToInstanceModal } =
    injectServerInstall()
const { install: installVersion, showSourceInstallModal, showSourceModpackInstall } = injectContentInstall()
const queryClient = useQueryClient()
const debugLog = useDebugLogger('Browse')

const router = useRouter()
const route = useRoute()
const displayedBrowseRoute = shallowRef(router.currentRoute.value)
watch(
    () => router.currentRoute.value,
    (nextRoute) => {
        if (nextRoute.path.startsWith('/browse/')) {
            displayedBrowseRoute.value = nextRoute
        }
    },
    { immediate: true },
)
const breadcrumbMessages = defineMessages({
    discoverProjectType: {
        id: 'app.browse.discover-project-type',
        defaultMessage: 'Discover {projectType}',
    },
    discoverServers: {
        id: 'app.browse.discover-servers',
        defaultMessage: 'Discover servers',
    },
})
const breadcrumbLabel = computed(() => {
    const browseRoute = displayedBrowseRoute.value
    if (browseRoute.query.from === 'worlds' || browseRoute.params.projectType === 'server') {
        return formatMessage(breadcrumbMessages.discoverServers)
    }

    return formatMessage(breadcrumbMessages.discoverProjectType, {
        projectType: formatProjectTypeSentence(
            formatMessage,
            String(browseRoute.params.projectType ?? ''),
            2,
        ),
    })
})
const appSettings = useAppSettings()
const browseRouteActive = computed(() => route.path.startsWith('/browse/'))
const { splitViewActive } = useSplitView()
/** Detail pane path segment, so tabs and links keep the open project. */
const splitDetailSuffix = computed(() => {
    if (!splitViewActive.value) return ''
    // A content source's project has its own segment pair; without this the
    // suffix would fall through to `/p/` and switching tabs would navigate to a
    // Modrinth route with an empty id. Both halves are re-encoded because the
    // params arrive decoded — a project id of `owner/repo` spliced in raw would
    // become two segments and match nothing.
    const source = String(route.params.sourceId ?? '')
    const project = String(route.params.projectId ?? '')
    if (source && project) {
        return `/s/${encodeURIComponent(source)}/${encodeURIComponent(project)}`
    }
    return `/p/${String(route.params.id ?? '')}`
})

/**
 * The open project's colour, washed across the detail pane.
 *
 * A project at full width tints the app's backdrop layer instead, but that
 * layer sits behind the discover list too, so in split view the pane itself is
 * the only thing on screen that belongs to the open project. Painted as the
 * pane's own background-image rather than an overlay, so it stays put while the
 * pane's content scrolls and can never cover it.
 *
 * The colour is published by whichever detail page is open: a content source
 * knows its own theme colour, and a Modrinth project carries one of its own.
 */
const splitPaneTint = computed(() => sourceTintStyle(splitDetailTintColor.value))
const serverSetupModalRef = ref<InstanceType<typeof CreationFlowModal> | null>(null)
const serverInstallContent = createServerInstallContent({ serverSetupModalRef })
provideServerInstallContent(serverInstallContent)
const {
    serverIdQuery,
    serverFlowFrom,
    isFromWorlds,
    isServerContext,
    isSetupServerContext,
    effectiveServerWorldId,
    serverContextServerData,
    serverContentProjectIds,
    queuedServerInstallRootProjectIds,
    queuedServerInstallProjectIds,
    queuedServerInstallCount,
    selectedServerInstallProjects,
    isInstallingQueuedServerInstalls,
    queuedInstallProgress,
    serverBackUrl,
    serverBackLabel,
    serverBrowseHeading,
    clearQueuedServerInstalls,
    removeQueuedServerInstall,
    flushQueuedServerInstalls,
    discardQueuedServerInstallsAndBack,
    installQueuedServerInstallsAndBack,
    initServerContext,
    watchServerContextChanges,
    searchServerModpacks,
    getServerProjectVersions,
    enforceSetupModpackRoute,
    getQueuedServerInstallPlans,
    setQueuedServerInstallPlans,
    resolveQueuedServerInstallPlan,
    openServerModpackInstallFlow,
    onServerFlowBack,
    handleServerModpackFlowCreate,
    markServerProjectInstalled,
} = serverInstallContent

const initialInstanceId = computed(() => String(route.query.i ?? ''))
const instanceQuery = useQuery(
    computed(() => ({
        ...instanceDetailQueryOptions(initialInstanceId.value),
        enabled: !!initialInstanceId.value,
    })),
)
const instance = computed(() => instanceQuery.data.value ?? null)
const linkedInstanceProjectId = computed(() => instance.value?.link?.project_id ?? '')
const linkedInstanceProjectQuery = useQuery(
    computed(() => ({
        ...instanceLinkedProjectQueryOptions(linkedInstanceProjectId.value),
        enabled: !!linkedInstanceProjectId.value,
    })),
)
const installedProjectIds: Ref<string[] | null> = ref(null)
const instanceHideInstalled = ref(route.query.ai === 'true')
const newlyInstalled = ref<string[]>([])
const hiddenInstanceProjectIds = ref<Set<string>>(new Set())
const hiddenInstanceProjectIdsInitialized = ref(false)
const isServerInstance = computed(
    () => linkedInstanceProjectQuery.data.value?.minecraft_server != null,
)

const breadcrumbManager = injectBreadcrumbManager()
const instanceBreadcrumbDefinition = {
    slot: 'instance',
    id: () => `instance:${String(displayedBrowseRoute.value.query.i ?? '')}`,
    label: () => instance.value?.name ?? formatMessage(commonMessages.loadingLabel),
    visual: () => ({
        type: 'image' as const,
        src: getInstanceIconUrl(instance.value?.icon_path),
        alt: instance.value?.name,
        tintBy: String(displayedBrowseRoute.value.query.i ?? ''),
    }),
    to: () => {
        const instancePath = `/instance/${encodeURIComponent(
            String(displayedBrowseRoute.value.query.i ?? ''),
        )}`
        return displayedBrowseRoute.value.query.from === 'worlds'
            ? `${instancePath}/worlds`
            : instancePath
    },
} satisfies BreadcrumbDefinition
const serversBreadcrumbDefinition = {
    slot: 'root',
    id: 'servers',
    label: () => formatMessage(commonMessages.serversLabel),
    to: '/hosting/manage/',
    visual: { type: 'icon', component: ServerStackIcon },
} satisfies BreadcrumbDefinition
const serverBreadcrumbTo = ref(serverBackUrl.value)
watch(serverBackUrl, (value) => {
    if (route.path.startsWith('/browse/')) {
        serverBreadcrumbTo.value = value
    }
})
const serverBreadcrumbDefinition = {
    slot: 'server',
    id: () => `server:${String(displayedBrowseRoute.value.query.sid ?? '')}`,
    label: () => serverContextServerData.value?.name ?? formatMessage(commonMessages.loadingLabel),
    visual: { type: 'icon', component: ServerStackIcon },
    to: serverBreadcrumbTo,
} satisfies BreadcrumbDefinition
const breadcrumbDefinition = {
    slot: 'browse',
    id: () =>
        `browse:${String(displayedBrowseRoute.value.params.projectType ?? '')}:${String(
            displayedBrowseRoute.value.query.i ?? '',
        )}:${String(displayedBrowseRoute.value.query.sid ?? '')}:${String(
            displayedBrowseRoute.value.query.from ?? '',
        )}`,
    label: breadcrumbLabel,
    // Always the list itself, never the detail pane opened on top of it.
    to: () => browseListLocation(displayedBrowseRoute.value),
    visual: { type: 'icon', component: CompassIcon },
} satisfies BreadcrumbDefinition

function syncBreadcrumbs() {
    if (displayedBrowseRoute.value.query.i) {
        const instanceBreadcrumb = breadcrumbManager.reset(instanceBreadcrumbDefinition)
        breadcrumbManager.push(breadcrumbDefinition, { parent: instanceBreadcrumb })
        return
    }

    if (displayedBrowseRoute.value.query.sid) {
        const serversBreadcrumb = breadcrumbManager.reset(serversBreadcrumbDefinition)
        const serverBreadcrumb = breadcrumbManager.push(serverBreadcrumbDefinition, {
            parent: serversBreadcrumb,
        })
        breadcrumbManager.push(breadcrumbDefinition, { parent: serverBreadcrumb })
        return
    }

    breadcrumbManager.reset(breadcrumbDefinition)
}

watch(
    // Keyed on what the crumb chain is built from, not on the whole route: a
    // reset would drop the project crumb the detail pane pushes on top of ours,
    // and query-only replaces (search, filters, paging) happen constantly.
    () =>
        [
            String(displayedBrowseRoute.value.params.projectType ?? ''),
            String(displayedBrowseRoute.value.query.i ?? ''),
            String(displayedBrowseRoute.value.query.sid ?? ''),
            String(displayedBrowseRoute.value.query.from ?? ''),
        ].join(':'),
    syncBreadcrumbs,
    { immediate: true, flush: 'sync' },
)

debugLog('fetching tags (categories, loaders, gameVersions)')
const [categories, loaders, availableGameVersions] = await Promise.all([
    get_categories()
        .catch(handleError)
        .then(ref<Labrinth.Tags.v2.Category[]>),
    get_loaders()
        .catch(handleError)
        .then(ref<Labrinth.Tags.v2.Loader[]>),
    get_game_versions()
        .catch(handleError)
        .then(ref<Labrinth.Tags.v2.GameVersion[]>),
])

const tags: Ref<Tags> = computed(() => ({
    gameVersions: availableGameVersions.value ?? [],
    loaders: loaders.value ?? [],
    categories: categories.value ?? [],
}))

/**
 * Section titles a source group declares use the same vocabulary the Modrinth
 * headers do, so they localize identically — a shader source's
 * `performance impact` header reads 性能要求, same as the built-in one. Anything
 * else capitalizes like an untranslated tag does.
 */
function formatSourceFilterHeader(header: string): string {
	const message = getCategoryHeaderMessage(header.trim().toLowerCase())
	return message ? formatMessage(message) : header.charAt(0).toUpperCase() + header.slice(1)
}

if (isFromWorlds.value && route.params.projectType !== 'server') {
    router.replace({
        path: browsePathFor('server', route),
        query: route.query,
    })
}

enforceSetupModpackRoute(route.params.projectType as string | undefined)

const allInstalledIds = computed(
    () => new Set([...newlyInstalled.value, ...(installedProjectIds.value ?? [])]),
)

function syncHiddenInstanceProjectIds() {
    hiddenInstanceProjectIds.value = new Set([
        ...(installedProjectIds.value ?? []),
        ...newlyInstalled.value,
    ])
    hiddenInstanceProjectIdsInitialized.value = true
}

watch(
    installedProjectIds,
    (ids) => {
        if (!ids) return
        if (!hiddenInstanceProjectIdsInitialized.value) {
            syncHiddenInstanceProjectIds()
        }
    },
    { immediate: true },
)

watchServerContextChanges()

await initInstanceContext()

async function refreshInstalledProjectIds() {
    if (!route.query.i) {
        const instances = await queryClient
            .fetchQuery({
                queryKey: [...instanceKeys.all, 'installed-project-ids'],
                queryFn: listInstances,
                staleTime: 0,
            })
            .catch(handleError)
        if (!instances) return

        const ids = instances
            .map((gameInstance) => gameInstance.link?.project_id)
            .filter((id): id is string => !!id)
        debugLog('installedInstanceProjectIds loaded', { count: ids.length })
        installedProjectIds.value = ids
        return
    }

    if (route.query.from === 'worlds') {
        const targetInstanceId = route.query.i as string
        const worlds = await queryClient
            .fetchQuery({
                queryKey: instanceKeys.installedProjectIds(targetInstanceId, 'worlds'),
                queryFn: () => get_instance_worlds(targetInstanceId),
                staleTime: 0,
            })
            .catch(handleError)
        if (!worlds) return

        const serverProjectIds = worlds
            .filter((w) => w.type === 'server' && 'project_id' in w && w.project_id)
            .map((w) => (w as { project_id: string }).project_id)
        debugLog('installedServerProjectIds loaded', { count: serverProjectIds.length })
        installedProjectIds.value = serverProjectIds
        return
    }

    const targetInstanceId = route.query.i as string
    const ids = await queryClient
        .fetchQuery({
            queryKey: instanceKeys.installedProjectIds(targetInstanceId, 'content'),
            queryFn: () => getInstalledProjectIds(targetInstanceId),
            staleTime: 0,
        })
        .catch(handleError)
    if (!ids) return

    debugLog('installedProjectIds loaded', { count: ids.length })
    installedProjectIds.value = ids
}

async function initInstanceContext() {
    debugLog('initInstanceContext', {
        queryI: route.query.i,
        queryAi: route.query.ai,
        querySid: route.query.sid,
        queryWid: route.query.wid,
        queryFrom: route.query.from,
    })
    await Promise.all([
        initServerContext(),
        refreshInstalledProjectIds(),
        route.query.i ? instanceQuery.suspense().catch(handleError) : Promise.resolve(),
    ])

    if (route.query.i) {
        debugLog('instance loaded', {
            name: instance.value?.name,
            loader: instance.value?.loader,
            gameVersion: instance.value?.game_version,
        })

        if (instance.value?.link?.project_id) {
            await linkedInstanceProjectQuery.suspense().catch(handleError)
        }
    }
}

function setBrowseHideInstalledFlag(flag: 'hide_installed_modpacks', value: boolean) {
    appSettings.featureFlags[flag] = value
    getSettings()
        .then((settings) => {
            settings.feature_flags[flag] = value
            return setSettings(settings)
        })
        .catch(handleError)
}

const hideInstalledModpacks = computed({
    get: () => appSettings.getFeatureFlag('hide_installed_modpacks'),
    set: (value: boolean) => setBrowseHideInstalledFlag('hide_installed_modpacks', value),
})

const instanceFilters = computed(() => {
    const filters = []

    if (instance.value && projectType.value !== 'resourcepack') {
        const isVanillaShader = projectType.value === 'shader' && instance.value.loader === 'vanilla'
        const gameVersion = instance.value.game_version
        if (gameVersion && !isVanillaShader) {
            filters.push({ type: 'game_version', option: gameVersion })
        }

        const platform = instance.value.loader
        const supportedModLoaders = ['fabric', 'forge', 'quilt', 'neoforge']

        if (platform && projectType.value === 'mod' && supportedModLoaders.includes(platform)) {
            filters.push({ type: 'mod_loader', option: platform })
        }
        if (isVanillaShader) {
            filters.push({ type: 'shader_loader', option: 'vanilla' })
        }

        if (isServerInstance.value) {
            filters.push({ type: 'environment', option: 'client' })
        }
    }

    if (
        (instance.value || projectType.value === 'modpack') &&
        (projectType.value === 'modpack' ? hideInstalledModpacks.value : instanceHideInstalled.value) &&
        hiddenInstanceProjectIds.value.size > 0
    ) {
        for (const id of hiddenInstanceProjectIds.value) {
            filters.push({ type: 'project_id', option: `project_id:${id}`, negative: true })
        }
    }

    return filters
})

const serverHideInstalled = ref(false)
const hideSelectedServerInstalls = ref(false)
if (route.query.shi) {
    serverHideInstalled.value = route.query.shi === 'true'
}
const hiddenServerContentProjectIds = ref<Set<string>>(new Set())
const hiddenServerContentProjectIdsInitialized = ref(false)

function syncHiddenServerContentProjectIds() {
    hiddenServerContentProjectIds.value = new Set(serverContentProjectIds.value)
    hiddenServerContentProjectIdsInitialized.value = true
}

watch(
    serverContentProjectIds,
    () => {
        if (!hiddenServerContentProjectIdsInitialized.value) {
            syncHiddenServerContentProjectIds()
        }
    },
    { immediate: true },
)

const serverContextFilters = computed(() => {
    const filters: { type: string; option: string; negative?: boolean }[] = []
    if (!serverContextServerData.value) return filters
    const pt = projectType.value

    if (pt !== 'modpack') {
        const gameVersion = serverContextServerData.value.mc_version
        if (gameVersion) filters.push({ type: 'game_version', option: gameVersion })

        const platform = serverContextServerData.value.loader?.toLowerCase()
        if (platform && ['fabric', 'forge', 'quilt', 'neoforge'].includes(platform))
            filters.push({ type: 'mod_loader', option: platform })
        if (platform && ['paper', 'purpur'].includes(platform))
            filters.push({ type: 'plugin_loader', option: platform })

        if (pt === 'mod') filters.push({ type: 'environment', option: 'server' })

        if (hideSelectedServerInstalls.value && queuedServerInstallProjectIds.value.size > 0) {
            for (const id of queuedServerInstallProjectIds.value) {
                filters.push({ type: 'project_id', option: `project_id:${id}`, negative: true })
            }
        }
    }

    if (pt === 'modpack') {
        filters.push(
            { type: 'environment', option: 'client' },
            { type: 'environment', option: 'server' },
        )

        if (hideInstalledModpacks.value && hiddenInstanceProjectIds.value.size > 0) {
            for (const id of hiddenInstanceProjectIds.value) {
                filters.push({ type: 'project_id', option: `project_id:${id}`, negative: true })
            }
        }
    }

    if (serverHideInstalled.value && hiddenServerContentProjectIds.value.size > 0) {
        for (const id of hiddenServerContentProjectIds.value) {
            filters.push({ type: 'project_id', option: `project_id:${id}`, negative: true })
        }
    }

    return filters
})

const combinedProvidedFilters = computed(() =>
    isServerContext.value ? serverContextFilters.value : instanceFilters.value,
)

const {
    serverPings,
    contextMenuRef,
    updateServerHits,
    getServerModpackContent,
    getServerCardActions,
    handleRightClick,
} = useAppServerBrowse({
    instance,
    isFromWorlds,
    allInstalledIds,
    newlyInstalled,
    installingServerProjects,
    playServerProject,
    showAddServerToInstanceModal,
    handleError,
    router,
})

const offline = ref(!navigator.onLine)
const handleOffline = () => {
    debugLog('went offline')
    offline.value = true
}
const handleOnline = () => {
    debugLog('went online')
    offline.value = false
}
window.addEventListener('offline', handleOffline)
window.addEventListener('online', handleOnline)

onBeforeUnmount(() => {
    window.removeEventListener('offline', handleOffline)
    window.removeEventListener('online', handleOnline)
})

const messages = defineMessages({
    addServersToInstance: {
        id: 'app.browse.add-servers-to-instance',
        defaultMessage: 'Adding server to instance',
    },
    projectActionsLabel: {
        id: 'app.browse.project-actions.label',
        defaultMessage: 'Project actions',
    },
    addToAnInstance: {
        id: 'app.browse.add-to-an-instance',
        defaultMessage: 'Add to an instance',
    },
    environmentProvidedByServer: {
        id: 'search.filter.locked.server-environment.title',
        defaultMessage: 'Only client-side mods can be added to the server instance',
    },
    gameVersionProvidedByInstance: {
        id: 'search.filter.locked.instance-game-version.title',
        defaultMessage: 'Game version is provided by the instance',
    },
    hideAddedServers: {
        id: 'app.browse.hide-added-servers',
        defaultMessage: 'Hide servers already added',
    },
    hideInstalledModpacks: {
        id: 'app.browse.hide-installed-modpacks',
        defaultMessage: 'Hide already installed',
    },
    installingToServer: {
        id: 'app.browse.server.installing',
        defaultMessage: 'Installing',
    },
    backToInstance: {
        id: 'app.browse.back-to-instance',
        defaultMessage: 'Back to instance',
    },
    serverInstanceContentWarning: {
        id: 'app.browse.server-instance-content-warning',
        defaultMessage:
            'Adding content may prevent you from joining this server. Any content you add will be removed when the managed server content is updated.',
    },
    modLoaderProvidedByInstance: {
        id: 'search.filter.locked.instance-loader.title',
        defaultMessage: 'Loader is provided by the instance',
    },
    modpacksProjectType: {
        id: 'app.browse.project-type.modpacks',
        defaultMessage: 'Modpacks',
    },
    modsProjectType: { id: 'app.browse.project-type.mods', defaultMessage: 'Mods' },
    resourcePacksProjectType: {
        id: 'app.browse.project-type.resource-packs',
        defaultMessage: 'Resource Packs',
    },
    dataPacksProjectType: {
        id: 'app.browse.project-type.data-packs',
        defaultMessage: 'Data Packs',
    },
    shadersProjectType: { id: 'app.browse.project-type.shaders', defaultMessage: 'Shaders' },
    serversProjectType: { id: 'app.browse.project-type.servers', defaultMessage: 'Servers' },
    worldsProjectType: { id: 'app.browse.project-type.worlds', defaultMessage: 'Worlds' },
    skinsProjectType: { id: 'app.browse.project-type.skins', defaultMessage: 'Skins' },
    providedByInstance: {
        id: 'search.filter.locked.instance',
        defaultMessage: 'Provided by the instance',
    },
    syncFilterButton: {
        id: 'search.filter.locked.instance.sync',
        defaultMessage: 'Sync with instance',
    },
})

const projectType = ref<ProjectType>(route.params.projectType as ProjectType)

function resetInstanceContext() {
    debugLog('instance context removed, resetting')
    installedProjectIds.value = null
    instanceHideInstalled.value = false
    newlyInstalled.value = []
    hiddenInstanceProjectIds.value = new Set()
    hiddenInstanceProjectIdsInitialized.value = false
    isServerInstance.value = false
    browseBreadcrumb.reset()
    void refreshInstalledProjectIds()
}

watch(
    () => route.params.projectType as ProjectType,
    async (newType) => {
        if (!browseRouteActive.value) {
            return
        }
        if (isSetupServerContext.value) {
            enforceSetupModpackRoute(newType)
            if (newType !== 'modpack') return
        }

        if (!newType || newType === projectType.value) return

        debugLog('projectType route param changed', { from: projectType.value, to: newType })
        projectType.value = newType
    },
)

watch(
    () => route.query.i,
    async (nextInstanceId, previousInstanceId) => {
        if (!route.path.startsWith('/browse') || nextInstanceId === previousInstanceId) return
        if (!nextInstanceId) {
            resetInstanceContext()
            return
        }

        installedProjectIds.value = null
        hiddenInstanceProjectIdsInitialized.value = false
        await Promise.all([instanceQuery.suspense().catch(handleError), refreshInstalledProjectIds()])
        if (instance.value?.link?.project_id) {
            await linkedInstanceProjectQuery.suspense().catch(handleError)
        }
    },
)

/**
 * Every content type the currently selected sources can serve. The tab row shows
 * exactly these: a type no selected source serves would only ever come back
 * empty, and a type missing from the row could not be reached at all.
 */
const activeContentTypes = computed<Set<string>>(() => {
    const types = new Set<string>()
    for (const source of selectableSources()) {
        if (!source.available || !isSourceActive(source.id)) continue
        for (const type of source.contentTypes) types.add(type)
    }
    return types
})

function hasSourceFor(type: ContentType): boolean {
    return activeContentTypes.value.has(type)
}

const selectableProjectTypes = computed(() => {
    let dataPacks = false,
        mods = false,
        modpacks = false

    if (instance.value) {
        if (
            availableGameVersions.value &&
            availableGameVersions.value.findIndex((x) => x.version === instance.value?.game_version) <=
            availableGameVersions.value.findIndex((x) => x.version === '1.13') &&
            !isServerInstance.value
        ) {
            dataPacks = true
        }

        if (instance.value.loader !== 'vanilla') {
            mods = true
        }
    } else {
        dataPacks = true
        mods = true
        modpacks = true
    }

    const params: LocationQuery = {}

    if (route.query.i) params.i = route.query.i
    if (route.query.ai) params.ai = route.query.ai
    if (route.query.from) params.from = route.query.from
    if (route.query.sid) params.sid = route.query.sid
    if (effectiveServerWorldId.value) params.wid = effectiveServerWorldId.value

    const queryString = new URLSearchParams(params as Record<string, string>).toString()
    const suffix = queryString ? `?${queryString}` : ''
    // Switching list type keeps the detail pane open in split view.
    const detail = splitDetailSuffix.value

    if (isSetupServerContext.value) {
        return [
            {
                label: formatMessage(messages.modpacksProjectType),
                href: `/browse/modpack${detail}${suffix}`,
            },
        ]
    }

    if (isFromWorlds.value) {
        return [
            {
                label: formatMessage(messages.serversProjectType),
                href: `/browse/server${detail}${suffix}`,
                shown: isSourceActive(MODRINTH_SOURCE_ID),
            },
        ]
    }

    return [
        {
            label: formatMessage(messages.modpacksProjectType),
            href: `/browse/modpack${detail}${suffix}`,
            shown: modpacks && hasSourceFor('modpack'),
        },
        {
            label: formatMessage(messages.modsProjectType),
            href: `/browse/mod${detail}${suffix}`,
            shown: mods && hasSourceFor('mod'),
        },
        {
            label: formatMessage(messages.resourcePacksProjectType),
            href: `/browse/resourcepack${detail}${suffix}`,
            shown: hasSourceFor('resourcepack'),
        },
        {
            label: formatMessage(messages.dataPacksProjectType),
            href: `/browse/datapack${detail}${suffix}`,
            shown: dataPacks && hasSourceFor('datapack'),
        },
        {
            label: formatMessage(messages.shadersProjectType),
            href: `/browse/shader${detail}${suffix}`,
            shown: hasSourceFor('shader'),
        },
        // A world is browsable with or without an instance; installing one
        // needs an instance, and the card says so when there is none. A skin
        // belongs to the launcher, so its tab is the other way round.
        {
            label: formatMessage(messages.worldsProjectType),
            href: `/browse/world${detail}${suffix}`,
            shown: hasSourceFor('world'),
        },
        {
            label: formatMessage(messages.skinsProjectType),
            href: `/browse/skin${detail}${suffix}`,
            shown: !instance.value && hasSourceFor('skin'),
        },
        {
            label: formatMessage(messages.serversProjectType),
            href: `/browse/server${detail}${suffix}`,
            // Server browsing is the built-in source's feature alone — no plugin
            // source serves it — so its tab follows the Modrinth toggle like the
            // content types it owns do.
            shown: !instance.value && isSourceActive(MODRINTH_SOURCE_ID),
        },
    ]
})

const installContext = computed(() => {
    if (isServerContext.value && serverContextServerData.value) {
        return {
            name: serverContextServerData.value.name,
            loader: serverContextServerData.value.loader ?? '',
            gameVersion: serverContextServerData.value.mc_version ?? '',
            serverId: serverIdQuery.value,
            upstream: serverContextServerData.value.upstream,
            iconSrc: null as string | null,
            isMedal: serverContextServerData.value.is_medal,
            backUrl: serverBackUrl.value,
            backLabel: serverBackLabel.value,
            heading: serverBrowseHeading.value,
            queuedCount: queuedServerInstallCount.value,
            selectedProjects: selectedServerInstallProjects.value,
            isInstallingSelected: isInstallingQueuedServerInstalls.value,
            skipNonEssentialWarnings: appSettings.getFeatureFlag('skip_non_essential_warnings'),
            installProgress: queuedInstallProgress.value,
            clearQueued: clearQueuedServerInstalls,
            clearSelected: clearQueuedServerInstalls,
            onBack: flushQueuedServerInstalls,
            discardSelectedAndBack: discardQueuedServerInstallsAndBack,
            installSelected: installQueuedServerInstallsAndBack,
        }
    }
    if (instance.value) {
        return {
            name: instance.value.name,
            loader: instance.value.loader,
            gameVersion: instance.value.game_version,
            iconSrc: getInstanceIconUrl(instance.value.icon_path),
            backUrl: `/instance/${encodeURIComponent(instance.value.id)}${isFromWorlds.value ? '/worlds' : ''}`,
            backLabel: formatMessage(messages.backToInstance),
            heading: formatMessage(
                isFromWorlds.value ? messages.addServersToInstance : commonMessages.installingContentLabel,
            ),
            warning:
                isServerInstance.value && instance.value.loader !== 'vanilla' && !isFromWorlds.value
                    ? formatMessage(messages.serverInstanceContentWarning)
                    : undefined,
        }
    }
    return null
})

const installingProjectIds = ref<Set<string>>(new Set())

function setProjectInstalling(projectId: string, installing: boolean) {
    const next = new Set(installingProjectIds.value)
    if (installing) {
        next.add(projectId)
    } else {
        next.delete(projectId)
    }
    installingProjectIds.value = next
}

const serverInstallQueue = {
    get: getQueuedServerInstallPlans,
    set: setQueuedServerInstallPlans,
}

function getCurrentSelectedInstallPreferences(projectTypeValue: string) {
    return getSelectedInstallPreferences({
        contentType: projectTypeValue,
        selectedFilters: searchState.currentFilters.value,
        providedFilters: combinedProvidedFilters.value,
        overriddenProvidedFilterTypes: searchState.overriddenProvidedFilterTypes.value,
    })
}

function getServerInstallTargetPreferences(contentType: BrowseInstallContentType) {
    return getTargetInstallPreferences(
        {
            gameVersion: serverContextServerData.value?.mc_version,
            loader: serverContextServerData.value?.loader,
        },
        contentType,
    )
}

function getInstanceInstallTargetPreferences(projectTypeValue: string) {
    return getTargetInstallPreferences(
        {
            gameVersion: instance.value?.game_version,
            loader: instance.value?.loader,
        },
        projectTypeValue,
    )
}

async function getInstallProjectVersions(projectId: string) {
    const project = await get_project(projectId, 'must_revalidate')
    return (await get_version_many(
        project.versions,
        'must_revalidate',
    )) as Labrinth.Versions.v2.Version[]
}

async function chooseInstanceInstallVersion(
    project: Labrinth.Search.v3.ResultSearchProject,
    projectTypeValue: string,
) {
    const targetInstance = instance.value
    if (!targetInstance) {
        return { versionId: null as string | null }
    }

    const selectedPreferences = getCurrentSelectedInstallPreferences(projectTypeValue)
    const targetPreferences = getInstanceInstallTargetPreferences(projectTypeValue)
    if (!preferencesDiffer(selectedPreferences, targetPreferences)) {
        return { versionId: null as string | null }
    }

    const selectedVersion = getLatestMatchingInstallVersion(
        await getInstallProjectVersions(project.project_id),
        selectedPreferences,
    )

    if (!selectedVersion) {
        return { versionId: null as string | null }
    }

    return { versionId: selectedVersion.id }
}

async function chooseFilterMatchingInstallVersion(
    project: Labrinth.Search.v3.ResultSearchProject,
    projectTypeValue: string,
) {
    const plan = await resolveInstallPlan({
        project: {
            project_id: project.project_id,
            title: project.title,
            icon_url: project.icon_url,
        },
        contentType: projectTypeValue as BrowseInstallContentType,
        selectedFilters: searchState.currentFilters.value,
        providedFilters: combinedProvidedFilters.value,
        overriddenProvidedFilterTypes: searchState.overriddenProvidedFilterTypes.value,
        targetPreferences: {},
        getProjectVersions: getInstallProjectVersions,
    })

    return { versionId: plan.versionId }
}

function getCardActions(
    result: Labrinth.Search.v3.ResultSearchProject,
    currentProjectType: string,
): CardAction[] {
    if (currentProjectType === 'server') {
        return getServerCardActions(result)
    }

    // A row from a content source has no Modrinth project behind it: it is
    // installed by asking its source for a download and handing that to the
    // launcher, which decides where the file goes.
    const foreign = (result as SourcedSearchResult).__source
    if (foreign) {
        const isInstalling = installingProjectIds.value.has(result.project_id)
        const installType = (result.project_types?.[0] ?? currentProjectType) as ContentType
        // Without an instance in context the click opens the instance picker,
        // exactly as a Modrinth row does, so the button is never dead.
        return [
            {
                key: 'install',
                label: formatMessage(
                    isInstalling ? messages.installingToServer : commonMessages.installButton,
                ),
                icon: isInstalling ? SpinnerIcon : PlusIcon,
                iconClass: isInstalling ? 'animate-spin' : undefined,
                disabled: isInstalling,
                color: 'brand',
                type: 'outlined',
                onClick: () => installForeignContent(result, foreign, installType),
            },
        ]
    }

    const projectResult = result as Labrinth.Search.v3.ResultSearchProject & {
        installed?: boolean
        installing?: boolean
    }
    const isInstalled =
        projectResult.installed ||
        allInstalledIds.value.has(projectResult.project_id || '') ||
        serverContentProjectIds.value.has(projectResult.project_id || '') ||
        serverContextServerData.value?.upstream?.project_id === projectResult.project_id
    const isInstalling = installingProjectIds.value.has(projectResult.project_id)
    const showAsInstalled = isInstalled && currentProjectType !== 'modpack'

    if (
        isServerContext.value &&
        ['modpack', 'mod', 'plugin', 'datapack'].includes(currentProjectType)
    ) {
        const isQueued = queuedServerInstallProjectIds.value.has(projectResult.project_id)
        const isQueuedRoot = queuedServerInstallRootProjectIds.value.has(projectResult.project_id)
        const isInstallingSelection = isInstallingQueuedServerInstalls.value
        const validatingInstall =
            isInstalling && currentProjectType !== 'modpack' && !isInstallingSelection
        const installLabel = showAsInstalled
            ? commonMessages.installedLabel
            : isQueued
                ? isInstalling || isInstallingSelection
                    ? validatingInstall
                        ? commonMessages.validatingLabel
                        : messages.installingToServer
                    : commonMessages.selectedLabel
                : isInstalling || isInstallingSelection
                    ? validatingInstall
                        ? commonMessages.validatingLabel
                        : messages.installingToServer
                    : commonMessages.installButton
        return [
            {
                key: 'install',
                label: formatMessage(installLabel),
                icon:
                    isInstalling || isInstallingSelection
                        ? SpinnerIcon
                        : isQueued || showAsInstalled
                            ? CheckIcon
                            : PlusIcon,
                iconClass: isInstalling || isInstallingSelection ? 'animate-spin' : undefined,
                disabled:
                    showAsInstalled || isInstalling || isInstallingSelection || (isQueued && !isQueuedRoot),
                color: isQueued && !isInstalling && !isInstallingSelection ? 'green' : 'brand',
                type: 'outlined',
                onClick: async () => {
                    if (isQueuedRoot) {
                        removeQueuedServerInstall(projectResult.project_id)
                        return
                    }
                    if (isQueued) return

                    const contentType = currentProjectType as BrowseInstallContentType
                    const isModpack = contentType === 'modpack'
                    const shouldShowInstalling = isModpack || !isQueued
                    if (shouldShowInstalling) {
                        setProjectInstalling(projectResult.project_id, true)
                    }
                    try {
                        const plan = await requestInstall({
                            project: projectResult,
                            contentType,
                            mode: isModpack ? 'immediate' : 'queue',
                            selectedFilters: isModpack
                                ? []
                                : stripServerRuntimeInstallFilters(searchState.currentFilters.value),
                            providedFilters: isModpack ? [] : combinedProvidedFilters.value,
                            overriddenProvidedFilterTypes: isModpack
                                ? []
                                : stripServerRuntimeInstallOverrides(
                                    searchState.overriddenProvidedFilterTypes.value,
                                ),
                            targetPreferences: getServerInstallTargetPreferences(contentType),
                            getProjectVersions: getInstallProjectVersions,
                            queue: serverInstallQueue,
                            install: (plan) =>
                                openServerModpackInstallFlow({
                                    projectId: plan.projectId,
                                    versionId: plan.versionId,
                                    name: plan.project.name,
                                    iconUrl: plan.project.icon_url ?? undefined,
                                }),
                        })
                        if (!isModpack) await resolveQueuedServerInstallPlan(plan)
                    } catch (err) {
                        if (!isModpack) removeQueuedServerInstall(projectResult.project_id)
                        handleError(err as Error)
                    } finally {
                        if (shouldShowInstalling) {
                            setProjectInstalling(projectResult.project_id, false)
                        }
                    }
                },
            },
        ]
    }

    const isModpack = projectResult.project_types?.includes('modpack')
    const shouldUseInstallIcon = !!instance.value || isModpack

    return [
        {
            key: 'install',
            label: formatMessage(
                isInstalling
                    ? messages.installingToServer
                    : showAsInstalled
                        ? commonMessages.installedLabel
                        : shouldUseInstallIcon
                            ? commonMessages.installButton
                            : messages.addToAnInstance,
            ),
            icon: isInstalling ? SpinnerIcon : showAsInstalled ? CheckIcon : PlusIcon,
            iconClass: isInstalling ? 'animate-spin' : undefined,
            disabled: showAsInstalled || isInstalling,
            color: 'brand',
            type: 'outlined',
            onClick: async () => {
                setProjectInstalling(projectResult.project_id, true)
                try {
                    const selectedInstall = instance.value
                        ? await chooseInstanceInstallVersion(projectResult, currentProjectType)
                        : isModpack
                            ? await chooseFilterMatchingInstallVersion(projectResult, currentProjectType)
                            : { versionId: null as string | null }
                    if (selectedInstall === null) {
                        setProjectInstalling(projectResult.project_id, false)
                        return
                    }
                    const selectedPreferences = getCurrentSelectedInstallPreferences(currentProjectType)
                    await installVersion(
                        projectResult.project_id,
                        selectedInstall.versionId,
                        instance.value ? instance.value.id : null,
                        'SearchCard',
                        (versionId, installedProjectIds) => {
                            setProjectInstalling(projectResult.project_id, false)
                            if (versionId) {
                                onSearchResultsInstalled(installedProjectIds ?? [projectResult.project_id])
                            }
                        },
                        (profile) => {
                            router.push(`/instance/${profile}`)
                        },
                        {
                            preferredLoader: instance.value?.loader ?? selectedPreferences.loaders?.[0],
                            preferredGameVersion:
                                instance.value?.game_version ?? selectedPreferences.gameVersions?.[0],
                        },
                    )
                } catch (err) {
                    setProjectInstalling(projectResult.project_id, false)
                    handleError(err)
                }
            },
        },
    ]
}

/**
/**
 * Install a row that came from a content source.
 *
 * With no instance in context this opens the same instance picker a Modrinth row
 * opens — a source does not know what an instance is, so choosing one stays the
 * launcher's job either way — and the install runs once a choice is made.
 */
function installForeignContent(
    result: Labrinth.Search.v3.ResultSearchProject,
    source: { sourceId: string; id: string },
    installType: ContentType,
) {
    const provider = sourceProvider(source.sourceId)
    if (!provider?.resolveDownload) {
        handleError(new Error('该内容源不支持直接下载。'))
        return
    }

    // A modpack *is* an instance: it runs the pack flow (stage, resolve, pick a
    // library, create) instead of picking an existing one. A skin belongs to
    // the launcher rather than to an instance and needs no picker at all.
    if (installType === 'modpack') {
        void showSourceModpackInstall({
                sourceId: source.sourceId,
                projectId: source.id,
                title: result.name,
                iconUrl: result.icon_url ?? undefined,
                callback: () => onSearchResultInstalled(result.project_id),
            })
            .catch((error: unknown) => handleError(toError(error)))
        return
    }
    if (installType !== 'skin' && !instance.value) {
        void showSourceInstallModal(
            (instanceId) => runForeignInstall(result, source, installType, instanceId),
            { title: result.name, iconUrl: result.icon_url ?? undefined },
        ).catch((error: unknown) => handleError(toError(error)))
        return
    }

    void runForeignInstall(result, source, installType, instance.value?.id)
}

/** Resolve the download and hand it to the launcher's install path. */
async function runForeignInstall(
    result: Labrinth.Search.v3.ResultSearchProject,
    source: { sourceId: string; id: string },
    installType: ContentType,
    targetInstanceId?: string,
) {
    const sourceName =
        selectableSources().find((candidate) => candidate.id === source.sourceId)?.name ??
        source.sourceId
    const provider = sourceProvider(source.sourceId)
    if (!provider?.resolveDownload) {
        throw new Error('该内容源不支持直接下载。')
    }

    setProjectInstalling(result.project_id, true)
    try {
        const descriptor = await provider.resolveDownload(source.id)
        const installed = await installSourceContent(
            targetInstanceId,
            installType,
            descriptor,
            {
                sourceId: source.sourceId,
                projectId: source.id,
                name: result.name,
                iconUrl: result.icon_url ?? undefined,
            },
        )
        void queryClient.invalidateQueries({ queryKey: instanceKeys.all })
        addNotification({
            title: `已安装「${result.name}」`,
            text: `来自内容源「${sourceName}」，位置：${installed.relative_path}`,
            type: 'success',
        })
    } catch (error) {
        throw toError(error)
    } finally {
        setProjectInstalling(result.project_id, false)
    }
}

function onSearchResultInstalled(id: string) {
    if (isServerContext.value) {
        markServerProjectInstalled(id)
        return
    }
    if (!newlyInstalled.value.includes(id)) {
        newlyInstalled.value = [...newlyInstalled.value, id]
    }
}

function onSearchResultsInstalled(ids: string[]) {
    if (isServerContext.value) {
        for (const id of ids) {
            markServerProjectInstalled(id)
        }
        return
    }
    newlyInstalled.value = Array.from(new Set([...newlyInstalled.value, ...ids]))
}

/** Pull the neutral request fields back out of the query string Modrinth got. */
function parseRequestParams(requestParams: string): {
    limit: number
    sort: ContentSort
    query: string
    page: number
} {
    const params = new URLSearchParams(
        requestParams.startsWith('?') ? requestParams.slice(1) : requestParams,
    )
    const limit = Number(params.get('limit')) || 20
    const sort = (params.get('index') ?? 'relevance') as ContentSort
    const query = params.get('query') ?? ''
    const offset = Number(params.get('offset')) || 0
    return { limit, sort, query, page: Math.floor(offset / limit) + 1 }
}

/** The game version and loader a source should filter by, when there is one. */
function queryContext(): { gameVersion?: string; loader?: string } {
    if (isServerContext.value) {
        return {
            gameVersion: serverContextServerData.value?.mc_version ?? undefined,
            loader: serverContextServerData.value?.loader ?? undefined,
        }
    }
    return {
        gameVersion: instance.value?.game_version ?? undefined,
        loader: instance.value?.loader ?? undefined,
    }
}

/**
 * Every source taking part in this request: the built-in Modrinth path when it
 * is enabled and serves the type, plus each content source that is loaded,
 * enabled and serves the type.
 */
function activeSourceQueries(requestParams: string, filtersBySource: Record<string, Record<string, string[]>> = {}): ActiveSourceQuery[] {
    const type = projectType.value
    const sources: ActiveSourceQuery[] = []
    const matchesSource = (sourceId: string) => searchState.filters.value.every((section) => {
        const selectedOptions = section.options.filter((option) =>
            (option.filter_values ?? [{ type: section.id, option: option.id }]).some((value) =>
                searchState.currentFilters.value.some((selected) =>
                    !selected.negative && selected.type === value.type && selected.option === value.option,
                ),
            ),
        )
        if (!selectedOptions.length || !section.options.some((option) =>
            option.filter_values?.some((value) => value.type.startsWith('source_')),
        )) return true
        return selectedOptions.some((option) =>
            (option.filter_values ?? [{ type: section.id, option: option.id }]).some((value) =>
                sourceId === MODRINTH_SOURCE_ID
                    ? !value.type.startsWith('source_')
                    : flattenSourceGroups(sourceFilterGroups.value).some((group) =>
                        group.sourceId === sourceId && group.filterTypeId === value.type,
                    ),
            ),
        )
    })

    // Modrinth only serves the five types it owns; a world or a skin is asked
    // of the plugin sources alone.
    if (
        isSourceActive(MODRINTH_SOURCE_ID) &&
        matchesSource(MODRINTH_SOURCE_ID) &&
        MODRINTH_CONTENT_TYPES.includes(type as ContentType)
    ) {
        sources.push({
            sourceId: MODRINTH_SOURCE_ID,
            query: () => searchModrinth(requestParams, type),
        })
    }

    for (const source of selectableSources()) {
        if (
            source.id === MODRINTH_SOURCE_ID ||
            !source.available ||
            !isSourceActive(source.id) ||
            !sourceSupportsType(source, type) ||
            !matchesSource(source.id)
        ) {
            continue
        }
        const provider = sourceProvider(source.id)
        if (!provider) continue
        const run = provider.search ?? provider.browse
        if (!run) continue
        const sourceFilters = filtersBySource[source.id]
        sources.push({
            sourceId: source.id,
            query: (request) =>
                run.call(provider, {
                    query: request.query,
                    contentType: request.contentType,
                    gameVersion: request.gameVersion,
                    loader: request.loader,
                    sort: request.sort,
                    page: request.page,
                    limit: request.limit,
                    filters: sourceFilters,
                } satisfies ContentQuery),
        })
    }

    return sources
}

async function searchModrinth(requestParams: string, contentType: string) {
    const rawResults = await queryClient.fetchQuery({
        queryKey: ['search', 'v3', requestParams],
        queryFn: () =>
            get_search_results_v3(requestParams, 'must_revalidate') as Promise<{
                result: Labrinth.Search.v3.SearchResults & {
                    hits: (Labrinth.Search.v3.ResultSearchProject & { installed?: boolean })[]
                }
            } | null>,
        staleTime: 30_000,
    })

    if (!rawResults) return { items: [], total: 0 }

    return {
        items: rawResults.result.hits.map((hit) => fromSearchResult(hit, contentType)),
        total: rawResults.result.total_hits ?? rawResults.result.hits.length,
    }
}

/** Sources that failed during the last search, so the notice is not repeated. */
let lastFailureKey = ''

function reportSourceFailures(failures: { sourceId: string; message: string }[]) {
    const key = failures
        .map((failure) => failure.sourceId)
        .sort()
        .join(',')
    if (key === lastFailureKey) return
    lastFailureKey = key

    for (const failure of failures) {
        const name =
            selectableSources().find((source) => source.id === failure.sourceId)?.name ??
            failure.sourceId
        handleError(new Error(`内容源「${name}」请求失败：${failure.message}`))
    }
}

async function search(requestParams: string) {
    debugLog('searching v3', requestParams)
    const isServer = projectType.value === 'server'

    // Server browsing has no content sources; it stays exactly as it was.
    if (isServer) {
        return searchServers(requestParams)
    }

    const request = parseRequestParams(requestParams)
    const context = queryContext()

    // The active source set is part of the cache key: toggling a source has to
    // produce a different list, and the multiplier that reorders it is folded in
    // too so a changed multiplier is not served a stale order. The sidebar's
    // source-namespace selections ride along too — they change what a plugin
    // source answers.
    const activeKey = selectableSources()
        .filter((source) => source.available && isSourceActive(source.id))
        .map((source) => `${source.id}:${sourceMultiplier(source.id)}`)
        .join(',')

    const sourceFilters = sourceFiltersFromSelection(
        flattenSourceGroups(sourceFilterGroups.value),
        searchState.currentFilters.value,
    )

    const merged = await queryClient.fetchQuery({
        queryKey: [
            'content-sources',
            'search',
            i18n.global.locale.value,
            activeKey,
            projectType.value,
            // The whole Modrinth request string, filters included: a tag the user
            // ticked in the sidebar changes `new_filters` inside it, and a key
            // that ignored that served the unfiltered list from cache.
            requestParams,
            context.gameVersion ?? '',
            context.loader ?? '',
            sourceFilters,
        ],
        // 120s: a content source is live-fetched over the network with no
        // backend cache of its own, so this is the only thing standing between a
        // tab switch and another round trip.
        staleTime: 120_000,
        queryFn: () =>
            searchAcrossSources(activeSourceQueries(requestParams, sourceFilters), {
                query: request.query,
                contentType: projectType.value as ContentQuery['contentType'],
                sort: request.sort,
                page: request.page,
                limit: request.limit,
                gameVersion: context.gameVersion,
                loader: context.loader,
                filters: sourceFilters,
            }),
    })

    reportSourceFailures(merged.failures)

    const sourceNames = new Map(
        selectableSources().map((source: SelectableSource) => [source.id, source.name]),
    )

    const hits = merged.items.map((item) => {
        // A Modrinth row is rendered from its original hit, which rides along on
        // the card so it survives the result cache; the neutral round trip drops
        // fields the card, the detail pane and the installer need.
        const raw = modrinthHitOf(item.card)
        if (raw) {
            // Re-seed the summary cache: on a cache hit the fetch that filled it
            // did not run, but the project page still reads from it.
            for (const identifier of [raw.project_id, raw.slug]) {
                if (identifier) queryClient.setQueryData(['projects', 'summary', identifier], raw)
            }
            return withInstalledFlag(raw)
        }

        const shared = item.offers.length > 1
        const others = item.offers
            .map((offer) => sourceNames.get(offer.sourceId) ?? offer.sourceId)
            .filter((name) => name !== sourceNames.get(item.card.sourceId))

        return toSearchResult(item.card, { shared, otherSources: others })
    })

    return {
        projectHits: hits,
        serverHits: [],
        total_hits: merged.total,
        per_page: request.limit,
    }
}

/** Stamp the installed badge onto a Modrinth hit, as the single-source path did. */
function withInstalledFlag(
    hit: Labrinth.Search.v3.ResultSearchProject & { installed?: boolean },
) {
    const mapped = { ...hit }
    if (instance.value || isServerContext.value || projectType.value === 'modpack') {
        const installedIds =
            isServerContext.value && projectType.value !== 'modpack'
                ? serverContentProjectIds.value
                : new Set([...newlyInstalled.value, ...(installedProjectIds.value ?? [])])
        mapped.installed = installedIds.has(hit.project_id)
    }
    return mapped
}

/** Server browsing, unchanged from the single-source path. */
async function searchServers(requestParams: string) {
    const rawResults = await queryClient.fetchQuery({
        queryKey: ['search', 'v3', requestParams],
        queryFn: () =>
            get_search_results_v3(requestParams, 'must_revalidate') as Promise<{
                result: Labrinth.Search.v3.SearchResults & {
                    hits: (Labrinth.Search.v3.ResultSearchProject & { installed?: boolean })[]
                }
            } | null>,
        staleTime: 30_000,
    })

    if (!rawResults) {
        return { projectHits: [], serverHits: [], total_hits: 0, per_page: 20 }
    }

    for (const hit of rawResults.result.hits) {
        for (const identifier of [hit.project_id, hit.slug]) {
            if (identifier) {
                queryClient.setQueryData(['projects', 'summary', identifier], hit)
            }
        }
    }

    const hits = rawResults.result.hits ?? []
    updateServerHits(hits)
    return {
        projectHits: [],
        serverHits: hits,
        total_hits: rawResults.result.total_hits ?? 0,
        per_page: rawResults.result.hits_per_page,
    }
}

const lockedFilterMessages = computed(() => ({
    gameVersion: formatMessage(messages.gameVersionProvidedByInstance),
    modLoader: formatMessage(messages.modLoaderProvidedByInstance),
    environment: formatMessage(messages.environmentProvidedByServer),
    syncButton: formatMessage(messages.syncFilterButton),
    providedBy: formatMessage(messages.providedByInstance),
}))

/**
 * The filter sections content sources contribute, rendered beside the Modrinth
 * ones. A source group whose header names a built-in section (`categories`,
 * `features`, `performance impact`) is merged into that section by
 * `flattenSourceGroups`, so e.g. pixelmap's map genres tick alongside Modrinth's
 * own categories rather than in a second "类别".
 */
const { groups: sourceFilterGroups } = useSourceFilters(() => projectType.value)

const extraFilterTypes = computed<FilterType[]>(() =>
    flattenSourceGroups(sourceFilterGroups.value).map((section: SidebarGroup) => ({
        id: section.filterTypeId,
        formatted_name: formatSourceFilterHeader(section.header),
        supported_project_types: [projectType.value as ProjectType],
        display: 'all',
        query_param: 'f',
        supports: ['include'],
        searchable: false,
        options: section.options.map((option) => ({
            id: option.id,
            formatted_name: option.label,
            method: 'or' as const,
            value: `${section.filterTypeId}:${option.id}`,
        })),
    })),
)

const searchState = useBrowseSearch({
    projectType,
    tags,
    active: browseRouteActive,
    providedFilters: combinedProvidedFilters,
    extraFilters: extraFilterTypes,
    search,
    persistentQueryParams: ['i', 'ai', 'shi', 'sid', 'wid', 'from'],
    getExtraQueryParams: () => ({
        sid: serverIdQuery.value || undefined,
        wid: effectiveServerWorldId.value || undefined,
        ai: instanceHideInstalled.value ? 'true' : undefined,
        shi: serverHideInstalled.value ? 'true' : undefined,
    }),
})

// Turning a source on or off (or changing a multiplier, which reorders the
// merged list) has to re-run the search; the shared search state cannot see the
// preference store on its own.
watch(() => i18n.global.locale.value, () => {
	void searchState.refreshSearch()
})

watch(sourcePreferences, () => {
    void searchState.refreshSearch()
})

// The shared search state refreshes off the Modrinth request string, and source
// filters deliberately stay out of it — they mean nothing to Modrinth. So their
// selections need their own trigger, or a tick in a source's group would render
// without ever re-asking that source.
const sourceFiltersSignature = computed(() =>
    JSON.stringify(
        sourceFiltersFromSelection(
            flattenSourceGroups(sourceFilterGroups.value),
            searchState.currentFilters.value,
        ),
    ),
)
watch(sourceFiltersSignature, () => {
    void searchState.refreshSearch()
})

watch(
    [
        () => searchState.query.value,
        () =>
            searchState.isServerType.value
                ? searchState.serverCurrentFilters.value
                : searchState.currentFilters.value,
        () => projectType.value,
    ],
    () => {
        if (isServerContext.value && projectType.value !== 'modpack') {
            syncHiddenServerContentProjectIds()
        } else if (instance.value || projectType.value === 'modpack') {
            syncHiddenInstanceProjectIds()
        }
    },
    { deep: true },
)

watch(queuedServerInstallCount, (count) => {
    if (count === 0) {
        hideSelectedServerInstalls.value = false
    }
})

if (instance.value?.game_version) {
    const gv = instance.value.game_version
    const alreadyHasGv = searchState.serverCurrentFilters.value.some(
        (f) => f.type === 'server_game_version' && f.option === gv,
    )
    if (!alreadyHasGv) {
        searchState.serverCurrentFilters.value.push({ type: 'server_game_version', option: gv })
    }
}

void searchState.refreshSearch()

useAppEvent('instance', async (event) => {
    if (event.event === 'created' || event.event === 'removed') {
        if (!route.query.i) {
            await refreshInstalledProjectIds()
            if (projectType.value === 'modpack') {
                if (event.event === 'removed') {
                    syncHiddenInstanceProjectIds()
                }
                await searchState.refreshSearch()
            }
        }
    }

    if (instance.value && event.instance_id === instance.value.id && event.event === 'synced') {
        await refreshInstalledProjectIds()
        await searchState.refreshSearch()
    }
})

function getProjectBrowseQuery() {
    if (!browseRouteActive.value) {
        return undefined
    }
    if (!installContext.value) return undefined
    return {
        ...route.query,
        b: route.fullPath,
    }
}

/**
 * In split view a card opens the nested detail route, so the list stays mounted
 * and the URL keeps describing both panes. The saved switch counts too: on the
 * full-width list a card click is what turns the split layout on, rather than
 * the list having to be split before anything is open.
 */
function getProjectDetailLocation(idOrSlug: string | undefined) {
    if (splitViewActive.value || openProjectsInSplitView.value) {
        return {
            path: `/browse/${String(route.params.projectType ?? projectType.value)}/p/${idOrSlug}`,
            query: browseListLocation(route).query,
        }
    }
    return {
        path: `/project/${idOrSlug}`,
        query: getProjectBrowseQuery(),
    }
}

/**
 * Where a card's detail link goes. A row from a content source has no Modrinth
 * project behind it, so it opens the source's own page instead.
 */
function projectLocationFor(result: Labrinth.Search.v3.ResultSearchProject) {
    const foreign = (result as SourcedSearchResult).__source
    if (!foreign) {
        return getProjectDetailLocation(result.project_id ?? result.slug)
    }

    const sourceId = encodeURIComponent(foreign.sourceId)
    const projectId = encodeURIComponent(foreign.id)
    // The content type lives in the path for the split route (it reuses the
    // browse `:projectType`), but the full-width route has no such segment, so
    // it rides in the query — the source page installs by type and must know it.
    const contentType = result.project_types?.[0] ?? projectType.value
    if (splitViewActive.value || openProjectsInSplitView.value) {
        return {
            path: `/browse/${String(route.params.projectType ?? projectType.value)}/s/${sourceId}/${projectId}`,
            query: browseListLocation(route).query,
        }
    }
    return {
        path: `/source/${sourceId}/${projectId}`,
        query: { ...getProjectBrowseQuery(), type: contentType },
    }
}

const advancedFiltersCollapsed = computed({
    get: () => appSettings.getFeatureFlag('advanced_filters_collapsed'),
    set: (value) => {
        appSettings.featureFlags['advanced_filters_collapsed'] = value
        getSettings()
            .then((settings) => {
                settings.feature_flags['advanced_filters_collapsed'] = value
                return setSettings(settings)
            })
            .catch(handleError)
    },
})

const dismissedPhotosensitivityFilterWarning = computed({
    get: () => appSettings.getFeatureFlag('dismissed_photosensitivity_filter_warning'),
    set: (value) => {
        appSettings.featureFlags['dismissed_photosensitivity_filter_warning'] = value
        getSettings()
            .then((settings) => {
                settings.feature_flags['dismissed_photosensitivity_filter_warning'] = value
                return setSettings(settings)
            })
            .catch(handleError)
    },
})

const { translate: translateContent } = useContentTranslation()

/*
 * Card summaries are translated here instead of inside `packages/ui`: the shared
 * layout renders whatever `projectHits` holds, and keys written after the
 * `...searchState` spread below override it, so the shared package stays untouched.
 * The clones are shallow and keep every id, so install and selection still match.
 */
function withTranslatedSummary<T extends { summary?: string | null }>(hit: T): T {
    if (!hit.summary) return hit

    const summary = translateContent(hit.summary)
    return summary === hit.summary ? hit : { ...hit, summary }
}

const translatedProjectHits = computed(() =>
    searchState.projectHits.value.map(withTranslatedSummary),
)
const translatedServerHits = computed(() => searchState.serverHits.value.map(withTranslatedSummary))

/**
 * Which source a card came from, for its tint and its chip. A merged card is
 * tinted by the source it is drawn from and says how many others also carry it,
 * so the chip does not silently hide the fact that a row is shared.
 */
function getCardSource(result: Labrinth.Search.v3.ResultSearchProject) {
    const foreign = (result as SourcedSearchResult).__source
    const appearance = sourceAppearance(foreign?.sourceId ?? MODRINTH_SOURCE_ID)
    const others = foreign?.otherSources?.length ?? 0
    return {
        label: others > 0 ? `${appearance.name} +${others}` : appearance.name,
        color: appearance.color,
        iconUrl: appearance.iconUrl,
        isModrinth: appearance.isModrinth,
    }
}

provideBrowseManager({
    tags,
    projectType,
    ...searchState,
    projectHits: translatedProjectHits,
    serverHits: translatedServerHits,
    advancedFiltersCollapsed,
    dismissedPhotosensitivityFilterWarning,
    getProjectLink: (result: Labrinth.Search.v3.ResultSearchProject) =>
        projectLocationFor(result),
    getServerProjectLink: (result: Labrinth.Search.v3.ResultSearchProject) =>
        getProjectDetailLocation(result.slug ?? result.project_id),
    getCardSource,
    selectableProjectTypes,
    showProjectTypeTabs: computed(() => !isServerContext.value),
    // Half the viewport is easily too narrow for every project type tab.
    scrollableProjectTypeTabs: computed(() => splitViewActive.value),
    variant: 'app',
    getCardActions,
    installContext,
    providedFilters: combinedProvidedFilters,
    hideInstalled: computed({
        get: () => {
            if (projectType.value === 'modpack') return hideInstalledModpacks.value
            if (isServerContext.value) return serverHideInstalled.value
            return instanceHideInstalled.value
        },
        set: (val: boolean) => {
            if (projectType.value === 'modpack') {
                hideInstalledModpacks.value = val
                if (val) syncHiddenInstanceProjectIds()
                return
            }
            if (isServerContext.value) {
                serverHideInstalled.value = val
                if (val) syncHiddenServerContentProjectIds()
            } else {
                instanceHideInstalled.value = val
                if (val) syncHiddenInstanceProjectIds()
            }
        },
    }),
    showHideInstalled: computed(
        () =>
            projectType.value === 'modpack' ||
            (isServerContext.value && projectType.value !== 'modpack') ||
            !!instance.value,
    ),
    hideInstalledLabel: computed(() =>
        formatMessage(
            isFromWorlds.value
                ? messages.hideAddedServers
                : projectType.value === 'modpack'
                    ? messages.hideInstalledModpacks
                    : commonMessages.hideInstalledContentLabel,
        ),
    ),
    hideSelected: hideSelectedServerInstalls,
    showHideSelected: computed(
        () =>
            isServerContext.value &&
            projectType.value !== 'modpack' &&
            queuedServerInstallCount.value > 0,
    ),
    hideSelectedLabel: computed(() => formatMessage(commonMessages.hideSelectedContentLabel)),
    onInstalled: onSearchResultInstalled,
    serverPings,
    getServerModpackContent,
    onContextMenu: handleRightClick,
    offline,
    lockedFilterMessages,
})
</script>

<template>
    <div class="browse-shell" :class="{ 'browse-shell--split': splitViewActive }">
        <div class="browse-list-pane">
            <div class="browse-list-content flex flex-col gap-3 p-6">
                <BrowsePageLayout>
                    <template #tabs-actions>
                        <SourceSelector />
                    </template>
                    <template #after>
                        <ContextMenu ref="contextMenuRef" :label="formatMessage(messages.projectActionsLabel)">
                            <template #open_link="{ option }">
                                <GlobeIcon /> {{ option.label }} <ExternalIcon />
                            </template>
                        </ContextMenu>
                    </template>
                </BrowsePageLayout>
                <CreationFlowModal
                    v-if="isServerContext && projectType === 'modpack'"
                    ref="serverSetupModalRef"
                    :type="serverFlowFrom === 'reset-server' ? 'reset-server' : 'server-onboarding'"
                    :available-loaders="['vanilla', 'fabric', 'neoforge', 'forge', 'quilt', 'paper', 'purpur']"
                    :show-snapshot-toggle="true"
                    :on-back="onServerFlowBack"
                    :search-modpacks="searchServerModpacks"
                    :get-project-versions="getServerProjectVersions"
                    :get-loader-manifest="getLoaderManifest"
                    @hide="() => {}"
                    @browse-modpacks="() => {}"
                    @create="handleServerModpackFlowCreate"
                />
            </div>
        </div>
        <div v-if="splitViewActive" class="browse-detail-pane bg-surface-1" :style="splitPaneTint">
            <RouterView v-slot="{ Component }">
                <template v-if="Component">
                    <Suspense>
                        <component :is="Component" />
                        <template #fallback>
                            <div class="flex items-center justify-center h-full">
                                <LoadingIndicator />
                            </div>
                        </template>
                    </Suspense>
                </template>
            </RouterView>
        </div>
        <Teleport v-if="browseRouteActive" to="#sidebar-teleport-target">
            <BrowseSidebar />
        </Teleport>
    </div>
</template>

<style scoped lang="scss">
/*
 * Without split view neither the shell nor the pane may exist as far as layout is
 * concerned: .browse-list-content stays the page's only box, exactly as before.
 */
.browse-shell,
.browse-list-pane {
    display: contents;
}

.browse-shell--split {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    /* Fills the viewport column (see App.vue) without pushing banners off screen. */
    flex: 1;
    min-height: 0;

    /*
     * Both panes scroll themselves so each scrollbar sits on its own inner edge;
     * .app-viewport gives up scrolling in split view (see App.vue).
     *
     * The list pane deliberately carries no padding of its own: `position: sticky`
     * resolves against its scroll container, so padding here would offset the
     * sticky install header downwards. The padding stays on .browse-list-content,
     * which keeps the header's geometry identical to the non-split layout.
     */
    .browse-list-pane,
    .browse-detail-pane {
        display: block;
        min-width: 0;
        height: 100%;
        overflow-y: auto;
        overflow-x: hidden;
        scrollbar-gutter: stable;
    }

    .browse-detail-pane {
        border-left: 1px solid var(--brand-gradient-border);
    }
}
</style>
