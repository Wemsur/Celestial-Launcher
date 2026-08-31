<template>
	<MultiStageModal
		ref="modal"
		:stages="ctx.stageConfigs"
		:context="ctx"
		:fade="fade"
		disable-progress
		@hide="handleHide"
	/>
</template>

<script setup lang="ts">
import { computed, useTemplateRef, watch } from 'vue'
import type { ComponentExposed } from 'vue-component-type-helpers'

import MultiStageModal from '../../base/MultiStageModal.vue'
import {
	createCreationFlowContext,
	type CreationFlowContextValue,
	type FlowType,
	type LoaderManifestResolver,
	type ProjectInstallCreateData,
	type ProjectInstallSelection,
	type ProjectSearchResult,
	provideCreationFlowContext,
} from './creation-flow-context'

const props = withDefaults(
	defineProps<{
		type?: FlowType
		availableLoaders?: string[]
		showSnapshotToggle?: boolean
		disableClose?: boolean
		isInitialSetup?: boolean
		initialLoader?: string
		initialGameVersion?: string
		fetchExistingInstanceNames?: () => Promise<string[]>
		onBack?: (() => void) | null
		fade?: 'standard' | 'warning' | 'danger'
		searchProjects?: (query: string, limit?: number) => Promise<ProjectSearchResult>
		prepareProjectInstall?: (
			projectId: string,
			projectType: string,
		) => Promise<ProjectInstallSelection | null>
		createProjectInstall?: (data: ProjectInstallCreateData) => Promise<void>
		getProjectVersions?: (projectId: string) => Promise<{ id: string }[]>
		getLoaderManifest?: LoaderManifestResolver
		randomizeInstanceIcon?: () => Promise<{ path: string; previewUrl: string } | null>
		customizeInstanceIcon?: () => void
		finishDisabled?: boolean
		finishDisabledTooltip?: string
		availableLibraries?: Array<{ path: string; name: string }>
		defaultLibraryPath?: string | null
		preselectedLibraryPath?: string | null
	}>(),
	{
		type: 'world',
		availableLoaders: () => ['fabric', 'neoforge', 'forge', 'quilt'],
		showSnapshotToggle: false,
		disableClose: false,
		isInitialSetup: false,
		initialLoader: undefined,
		initialGameVersion: undefined,
		fetchExistingInstanceNames: undefined,
		onBack: null,
		randomizeInstanceIcon: undefined,
		customizeInstanceIcon: undefined,
	},
)

const emit = defineEmits<{
	(e: 'hide' | 'browse-modpacks'): void
	(e: 'create', config: CreationFlowContextValue): void
}>()

const modal = useTemplateRef<ComponentExposed<typeof MultiStageModal>>('modal')

const ctx = createCreationFlowContext(
	modal,
	props.type,
	{
		browseModpacks: () => emit('browse-modpacks'),
		create: (config) => emit('create', config),
	},
	{
		availableLoaders: props.availableLoaders,
		showSnapshotToggle: props.showSnapshotToggle,
		disableClose: props.disableClose,
		isInitialSetup: props.isInitialSetup,
		initialLoader: props.initialLoader,
		initialGameVersion: props.initialGameVersion,
		fetchExistingInstanceNames: props.fetchExistingInstanceNames,
		onBack: props.onBack ?? undefined,
		searchProjects: props.searchProjects,
		prepareProjectInstall: props.prepareProjectInstall,
		createProjectInstall: props.createProjectInstall,
		getProjectVersions: props.getProjectVersions,
		getLoaderManifest: props.getLoaderManifest,
		randomizeInstanceIcon: props.randomizeInstanceIcon,
		customizeInstanceIcon: props.customizeInstanceIcon,
		finishDisabled: computed(() => props.finishDisabled ?? false),
		finishDisabledTooltip: computed(() => props.finishDisabledTooltip),
		availableLibraries: props.availableLibraries ?? [],
		defaultLibraryPath: props.defaultLibraryPath,
		preselectedLibraryPath: props.preselectedLibraryPath ?? null,
	},
)
provideCreationFlowContext(ctx)

// When preselectedLibraryPath changes, update selectedLibraryPath immediately
// so it's correct both while the modal is open and on next show()
watch(() => props.preselectedLibraryPath, (value) => {
	ctx.selectedLibraryPath.value = resolveLibraryPath(value)
})

// The default library is resolved asynchronously by the caller, so it can land
// after this component was created; the picker labels it from this ref.
watch(() => props.defaultLibraryPath, (value) => {
	ctx.defaultLibraryPath.value = value ?? null
})

/**
 * A preselection is only usable if the picker actually offers it: anything else
 * leaves the box looking empty while counting as a selection, which would let
 * the create button through with a library that does not exist.
 *
 * `'all'` is the home page's "全部实例" tab, i.e. no specific library, not a path.
 */
function resolveLibraryPath(path: string | null | undefined): string | null {
	if (!path || path === 'all') return null
	return ctx.availableLibraries.value.some((library) => library.path === path) ? path : null
}

function storedLibraryPath(): string | null {
	if (typeof window === 'undefined') return null
	return localStorage.getItem('celestial-library-active-tab')
}

/** `preselectedLibrary` wins over the prop: the caller resolves it as the user
 *  clicks, and a prop written in that same tick has not reached us yet. */
async function show(preselectedLibrary?: string | null) {
	// Sync the freshest props into ctx BEFORE reset so both the library list
	// and preselected path are always up-to-date on each open.
	ctx.availableLibraries.value = props.availableLibraries ?? []
	ctx.defaultLibraryPath.value = props.defaultLibraryPath ?? null
	const preselected = resolveLibraryPath(
		preselectedLibrary !== undefined
			? preselectedLibrary
			: (props.preselectedLibraryPath ?? storedLibraryPath()),
	)
	ctx.selectedLibraryPath.value = preselected
	await ctx.reset()
	// `reset()` restores the preselection captured when the context was created,
	// which is stale by now, so re-apply the freshest one afterwards.
	ctx.selectedLibraryPath.value = preselected
	void ctx.prefetchLoaderMetadata()
	modal.value?.setStage(0)
	modal.value?.show()
}

function hide() {
	modal.value?.hide()
}

function handleHide() {
	ctx.cancelBackup.value?.()
	emit('hide')
}

defineExpose({ show, hide, ctx })
</script>
