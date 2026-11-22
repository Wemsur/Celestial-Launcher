import type { ContentUpdaterVersion } from '../../../types'
import type { ComputedRef, Ref } from 'vue'
import { computed } from 'vue'

type UseContentUpdaterFilteringOptions<T extends ContentUpdaterVersion> = {
	versions: Readonly<Ref<T[]>>
	searchQuery: Ref<string>
	isModpack: ComputedRef<boolean>
	incompatibilityWarningMode: ComputedRef<boolean>
	hideIncompatibleState: Ref<boolean>
	selectedVersion: Ref<T | null>
	pinnedInitialVersionId: Ref<string | undefined>
	currentVersionId: Readonly<Ref<string>>
	isVersionCompatible: (version: T) => boolean
	debug: (message: string, data?: Record<string, unknown>) => void
}

export function useContentUpdaterFiltering<T extends ContentUpdaterVersion>({
	versions: sourceVersions,
	searchQuery,
	isModpack,
	incompatibilityWarningMode,
	hideIncompatibleState,
	selectedVersion,
	pinnedInitialVersionId,
	currentVersionId,
	isVersionCompatible,
	debug,
}: UseContentUpdaterFilteringOptions<T>) {
	return computed(() => {
		let versions = [...sourceVersions.value]

		if (searchQuery.value) {
			const query = searchQuery.value.toLowerCase()
			versions = versions.filter(
				(v) =>
					v.name.toLowerCase().includes(query) ||
					v.version_number.toLowerCase().includes(query) ||
					(incompatibilityWarningMode.value &&
						[...v.loaders, ...v.game_versions].some((value) =>
							value.toLowerCase().includes(query),
						)),
			)
		}

		const beforeFilterCount = versions.length
		if (!incompatibilityWarningMode.value && !isModpack.value && hideIncompatibleState.value) {
			versions = versions.filter(
				(version) =>
					version.compatibility_known === false ||
					version.id === currentVersionId.value ||
					version.id === selectedVersion.value?.id ||
					version.id === pinnedInitialVersionId.value ||
					isVersionCompatible(version),
			)
		}

		debug('filteredVersions computed', {
			totalVersions: sourceVersions.value.length,
			afterSearchFilter: beforeFilterCount,
			afterCompatibilityFilter: versions.length,
			hiddenByCompatibility: beforeFilterCount - versions.length,
			hideIncompatible: hideIncompatibleState.value,
			filteringCompatibility: !isModpack.value && hideIncompatibleState.value,
		})

		return versions
	})
}
