<template>
	<div>
		<ProjectPageVersions
			:loaders="loaders"
			:game-versions="gameVersions"
			:versions="rows"
			:project="tableProject"
			:version-link="(version) => versionHref(version)"
			@row-context-menu="openRowMenu"
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
				<IconButton
					v-if="versionUrl(version)"
					v-tooltip="formatMessage(commonMessages.openInBrowserButton)"
					type="quiet"
					:label="formatMessage(commonMessages.openInBrowserButton)"
					@click.stop="openVersionUrl(version)"
				>
					<ExternalIcon />
				</IconButton>
			</template>
		</ProjectPageVersions>

		<!-- Right-clicking a row offers what the row can do but the table has no
		     room to show; today that is only opening the version on its own site,
		     which a source need not have. -->
		<ContextMenu ref="rowMenu" @option-clicked="onRowMenuOption">
			<template #open_link>
				<ExternalIcon />
				{{ formatMessage(messages.openLink) }}
			</template>
		</ContextMenu>
	</div>
</template>

<script setup lang="ts">
import { DownloadIcon, ExternalIcon, SpinnerIcon } from '@modrinth/assets'
import {
	commonMessages,
	defineMessages,
	IconButton,
	injectNotificationManager,
	ProjectPageVersions,
	useVIntl,
} from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'

import ContextMenu from '@/components/ui/ContextMenu.vue'
import { sourceProjectBasePathOf } from '@/composables/use-split-view.ts'
import { useSourceProject } from '@/composables/use-source-project'
import { toVersionRow } from '@/content-sources/adapter'
import { toError } from '@/helpers/errors'
import { get_game_versions, get_loaders } from '@/helpers/tags.js'

const route = useRoute()
const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()
const { projectId, contentType, provider, versions, installing, install } = useSourceProject()

const messages = defineMessages({
	openLink: {
		id: 'app.source-project.versions.open-link',
		defaultMessage: 'Open on the source',
	},
})

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

/** The table hands back its own row shape; only these fields are read here. */
type VersionRef = { id: string; name?: string; url?: string }

function versionHref(version: VersionRef) {
	return `${sourceProjectBasePathOf(route)}/version/${encodeURIComponent(version.id)}`
}

function versionUrl(version: VersionRef): string | undefined {
	return version.url
}

function openVersionUrl(version: VersionRef) {
	if (version.url) void openUrl(version.url)
}

const rowMenu = ref<InstanceType<typeof ContextMenu> | null>(null)
/** The row the open menu belongs to. */
let menuTarget: VersionRef | null = null

function openRowMenu(version: VersionRef, event: MouseEvent) {
	if (!versionUrl(version)) return
	menuTarget = version
	rowMenu.value?.showMenu(event, version, [{ name: 'open_link' }])
}

function onRowMenuOption(name: string) {
	if (name === 'open_link' && menuTarget) openVersionUrl(menuTarget)
	menuTarget = null
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
