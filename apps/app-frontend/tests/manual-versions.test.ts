import assert from 'node:assert/strict'
import test from 'node:test'

import { sourcePickerVersion } from '../src/content-sources/manual-versions'
import { useContentUpdaterSelection } from '../../../packages/ui/src/layouts/shared/content-tab/components/modals/content-updater-modal/use-content-updater-selection'
import { useContentUpdaterFiltering } from '../../../packages/ui/src/layouts/shared/content-tab/components/modals/content-updater-modal/use-content-updater-filtering'
import type { ContentUpdaterVersion } from '../../../packages/ui/src/layouts/shared/content-tab/types'
import { computed, effectScope, nextTick, ref } from 'vue'

test('source picker keeps missing dates and compatibility unknown', () => {
	const version = sourcePickerVersion({ id: 'opaque-version' }, 'mod')
	assert.equal(version.date_published, null)
	assert.equal(version.version_type, null)
	assert.equal(version.compatibility_known, false)
	assert.deepEqual(version.game_versions, [])
	assert.deepEqual(version.loaders, [])
})

test('datapacks without loader metadata are not declared compatible', () => {
	const version = sourcePickerVersion({ id: 'v1', gameVersions: ['1.21'] }, 'datapack')
	assert.equal(version.compatibility_known, false)
})

test('async picker selects a version even when installed version is unknown', async () => {
	const scope = effectScope()
	try {
		const versions = ref<ContentUpdaterVersion[]>([])
		const selected: string[] = []
		const selection = scope.run(() => useContentUpdaterSelection({
			versions, currentVersionId: ref(''), onVersionSelect: (version) => selected.push(version.id), debug: () => {},
		}))!
		selection.resetInitialSelection()
		versions.value = [sourcePickerVersion({ id: 'v1' }, 'mod')]
		await nextTick()
		assert.equal(selection.selectedVersion.value?.id, 'v1')
		assert.deepEqual(selected, ['v1'])
	} finally { scope.stop() }
})

test('hide incompatible keeps unknown metadata selectable without claiming compatibility', () => {
	const versions = ref<ContentUpdaterVersion[]>([
		sourcePickerVersion({ id: 'unknown' }, 'mod'),
		{ ...sourcePickerVersion({ id: 'known-incompatible', gameVersions: ['1.20'], loaders: ['fabric'] }, 'mod') },
	])
	const filtered = useContentUpdaterFiltering({
		versions, searchQuery: ref(''), isModpack: computed(() => false), incompatibilityWarningMode: computed(() => false),
		hideIncompatibleState: ref(true), selectedVersion: ref<ContentUpdaterVersion | null>(null),
		pinnedInitialVersionId: ref<string | undefined>(), currentVersionId: ref(''),
		isVersionCompatible: () => false, debug: () => {},
	})
	assert.deepEqual(filtered.value.map((version) => version.id), ['unknown'])
})
