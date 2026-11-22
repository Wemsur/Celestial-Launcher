import type { Ref } from 'vue'
import { ref, shallowRef, watch } from 'vue'

import type { ContentUpdaterVersion } from '../../../types'

type UseContentUpdaterSelectionOptions<T extends ContentUpdaterVersion> = {
	versions: Readonly<Ref<T[]>>
	currentVersionId: Readonly<Ref<string>>
	onVersionSelect: (version: T) => void
	debug: (message: string, data?: Record<string, unknown>) => void
}

export function useContentUpdaterSelection<T extends ContentUpdaterVersion>({
	versions,
	currentVersionId,
	onVersionSelect,
	debug,
}: UseContentUpdaterSelectionOptions<T>) {
	const selectedVersion = shallowRef<T | null>(null)
	const pendingInitialVersionId = ref<string | undefined>(undefined)
	const waitingForVersions = ref(false)
	const pinnedInitialVersionId = ref<string | undefined>(undefined)

	watch(
		versions,
		(newVersions) => {
			if (selectedVersion.value) {
				const updatedVersion = newVersions.find((v) => v.id === selectedVersion.value?.id)
				if (updatedVersion && updatedVersion !== selectedVersion.value) {
					selectedVersion.value = updatedVersion
				}
			}

			if (newVersions.length > 0 && !selectedVersion.value && waitingForVersions.value) {
				const pendingFound = newVersions.find((v) => v.id === pendingInitialVersionId.value)
				debug('versions watcher: initial selection', {
					pendingInitialVersionId: pendingInitialVersionId.value,
					foundPending: !!pendingFound,
					currentVersionId: currentVersionId.value,
					currentInList: newVersions.some((v) => v.id === currentVersionId.value),
					totalVersions: newVersions.length,
					loaderDistribution: [...new Set(newVersions.flatMap((v) => v.loaders))],
					gameVersionDistribution: [...new Set(newVersions.flatMap((v) => v.game_versions))].slice(
						0,
						10,
					),
				})
				const version = pendingFound ?? newVersions[0]
				selectedVersion.value = version
				if (version) {
					onVersionSelect(version)
				}
				pendingInitialVersionId.value = undefined
				waitingForVersions.value = false
			}
		},
		{ deep: true },
	)

	function selectVersion(version: T) {
		selectedVersion.value = version
		onVersionSelect(version)
	}

	function resetInitialSelection(initialVersionId?: string) {
		pinnedInitialVersionId.value = initialVersionId
		waitingForVersions.value = versions.value.length === 0

		if (versions.value.length > 0) {
			selectedVersion.value = initialVersionId
				? (versions.value.find((v) => v.id === initialVersionId) ?? versions.value[0])
				: versions.value[0]
			pendingInitialVersionId.value = undefined
			if (selectedVersion.value) {
				onVersionSelect(selectedVersion.value)
			}
		} else {
			selectedVersion.value = null
			pendingInitialVersionId.value = initialVersionId
			debug('show(): no versions yet, deferring selection', {
				pendingInitialVersionId: initialVersionId,
			})
		}
	}

	return {
		selectedVersion,
		pinnedInitialVersionId,
		selectVersion,
		resetInitialSelection,
	}
}
