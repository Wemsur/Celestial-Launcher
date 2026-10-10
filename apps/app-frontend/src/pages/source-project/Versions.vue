<template>
	<div>
		<ProjectPageVersions
			:loaders="loaders"
			:game-versions="gameVersions"
			:versions="rows"
			:project="tableProject"
			:version-link="(version) => versionHref(version)"
		>
			<template #actions="{ version }">
				<IconButton
					v-if="provider?.resolveDownload"
					v-tooltip="formatMessage(commonMessages.installButton)"
					type="quiet"
					color="green"
					:label="formatMessage(commonMessages.installButton)"
					:disabled="installing"
					@click.stop="installVersion(version)"
				>
					<SpinnerIcon v-if="installing" class="animate-spin" />
					<DownloadIcon v-else />
				</IconButton>
			</template>
		</ProjectPageVersions>
	</div>
</template>

<script setup lang="ts">
import { DownloadIcon, SpinnerIcon } from '@modrinth/assets'
import {
	commonMessages,
	IconButton,
	injectNotificationManager,
	ProjectPageVersions,
	useVIntl,
} from '@modrinth/ui'
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'

import { sourceProjectBasePathOf } from '@/composables/use-split-view.ts'
import { useSourceProject } from '@/composables/use-source-project'
import { toVersionRow } from '@/content-sources/adapter'
import { toError } from '@/helpers/errors'
import { get_game_versions, get_loaders } from '@/helpers/tags.js'

const route = useRoute()
const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()
const { projectId, contentType, provider, versions, installing, install } = useSourceProject()

/*
 * Modrinth's own versions table, fed a source's versions: the same columns,
 * filters, grouping and row actions, so a source's page is the same page rather
 * than a second list that drifts from it.
 */
const rows = computed(() => versions.value.map(toVersionRow))

/** Only `project_type` and `id` are read — for the modpack loader rule. */
const tableProject = computed(() => ({
	project_type: String(contentType.value),
	id: projectId.value,
}))

/** The table hands back its own row shape; these are the two fields read here. */
type VersionRef = { id: string; name?: string }

function versionHref(version: VersionRef) {
	return `${sourceProjectBasePathOf(route)}/version/${encodeURIComponent(version.id)}`
}

async function installVersion(version: VersionRef) {
	try {
		await install({ versionId: version.id, versionName: version.name })
	} catch (caught) {
		handleError(toError(caught))
	}
}

// The tag lists the table's filters offer. Modrinth's page reads the same two,
// and a source's own values are matched against them by name.
const [loaders, gameVersions] = await Promise.all([
	get_loaders().catch(handleError).then(ref),
	get_game_versions().catch(handleError).then(ref),
])
</script>
