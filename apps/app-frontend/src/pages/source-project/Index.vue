<template>
	<div class="flex flex-col gap-4 p-6">
		<!--
			The source's colour as the page's backdrop, in the same layer the
			Modrinth project background uses: behind everything, so it can never
			come between the reader and the text, and stationary while the content
			scrolls over it.

			The element carries its own size — a bare div with only a background
			has no box to paint, which is how the first attempt rendered nothing.
			The height matches `ProjectBackgroundGradient`'s rather than using
			`h-full`: that component is known to work in this layer, and a
			percentage height would collapse to nothing if the backdrop's own
			height is not definite. The colour fades out by half of it.

			Not gated on the `project_background` flag the way Modrinth's is: that
			flag defaults to off, and a source page whose whole visual identity is
			its source's colour should not depend on a setting most users never
			touch.
		-->
		<Teleport v-if="!splitViewActive" to="#background-teleport-target">
			<div class="h-[60rem] w-full" :style="tintStyle" />
		</Teleport>

		<ProjectPageHeader
			v-if="project"
			:project="headerProject"
			:project-v3="headerProjectV3"
			stack-actions-when-narrow
			@category="(category) => router.push(`${browsePath}?f=categories:${category}`)"
		>
			<template #actions>
				<Button
					v-if="installed"
					v-tooltip="formatMessage(messages.installedHint)"
					size="xl"
					native-type="button"
					disabled
				>
					<CheckIcon />
					{{ formatMessage(commonMessages.installedLabel) }}
				</Button>
				<Button
					v-else
					type="colored"
					color="brand"
					size="xl"
					native-type="button"
					:disabled="installing || !provider?.resolveDownload"
					@click="runInstall()"
				>
					<SpinnerIcon v-if="installing" class="animate-spin" />
					<DownloadIcon v-else />
					{{
						installing
							? formatMessage(commonMessages.installingLabel)
							: formatMessage(commonMessages.installButton)
					}}
				</Button>
				<TeleportOverflowMenu
					type="quiet"
					size="xl"
					:label="formatMessage(messages.moreOptions)"
					:options="moreActions"
				>
					<MoreVerticalIcon />
				</TeleportOverflowMenu>
			</template>
		</ProjectPageHeader>

		<template v-if="project">
			<NavTabs :links="tabLinks" />
			<RouterView />
		</template>
		<Card v-else-if="error">
			<p class="m-0 text-secondary">{{ error }}</p>
		</Card>
		<Card v-else>
			<p class="m-0 text-secondary">{{ formatMessage(messages.loading) }}</p>
		</Card>
	</div>

	<!--
		The project's own sections. In split view there is no room for a second
		sidebar, so they render inline at the bottom of the detail pane instead —
		the same arrangement the Modrinth project page uses.
	-->
	<Teleport to="#sidebar-teleport-target" :disabled="splitViewActive">
		<div :class="splitViewActive ? 'split-project-sidebar bg-surface-1' : 'contents'">
			<div v-if="project?.links?.length" class="project-sidebar-section">
				<h2 class="m-0 text-lg">{{ formatMessage(messages.linksTitle) }}</h2>
				<div class="flex flex-col gap-1">
					<button
						v-for="link in project.links"
						:key="link.url"
						type="button"
						class="flex cursor-pointer items-center gap-2 border-0 bg-transparent p-0 text-left text-primary hover:underline"
						@click="openLink(link.url)"
					>
						<LinkIcon class="shrink-0" aria-hidden="true" />
						<span class="min-w-0 truncate">{{ link.label }}</span>
						<ExternalIcon class="shrink-0" aria-hidden="true" />
					</button>
				</div>
			</div>

			<div v-if="project?.tags?.length" class="project-sidebar-section">
				<h2 class="m-0 text-lg">{{ formatMessage(messages.tagsTitle) }}</h2>
				<div class="flex flex-wrap gap-1">
					<TagItem v-for="tag in project.tags" :key="tag">{{ tag }}</TagItem>
				</div>
			</div>

			<div v-if="project?.author" class="project-sidebar-section">
				<h2 class="m-0 text-lg">{{ formatMessage(messages.authorTitle) }}</h2>
				<div class="flex items-center gap-2">
					<UserIcon class="shrink-0" aria-hidden="true" />
					<span class="min-w-0 truncate text-primary">{{ project.author }}</span>
				</div>
			</div>

			<div class="project-sidebar-section">
				<h2 class="m-0 text-lg">
					{{ formatMessage(commonMessages.detailsLabel) }}
				</h2>
				<div
					class="flex flex-col gap-3 [&>div>svg]:mt-[1px] [&>div>svg]:shrink-0 [&>div]:flex [&>div]:items-start [&>div]:gap-2 [&>div>div]:min-w-0"
				>
					<div v-if="project?.downloads !== undefined">
						<DownloadIcon aria-hidden="true" />
						<div class="flex flex-col">
							<span>
								{{ formatNumber(project.downloads) }}
								{{ formatMessage(messages.downloadsLabel) }}
							</span>
						</div>
					</div>
					<div v-if="project?.follows !== undefined">
						<HeartIcon aria-hidden="true" />
						<div class="flex flex-col">
							<span>
								{{ formatNumber(project.follows) }}
								{{ formatMessage(messages.followersLabel) }}
							</span>
						</div>
					</div>
					<div v-if="project?.publishedAt">
						<CalendarIcon aria-hidden="true" />
						<div class="flex flex-col">
							<span>
								{{ formatMessage(messages.publishedLabel) }}
								{{ formatDate(project.publishedAt) }}
							</span>
						</div>
					</div>
					<div v-if="project?.updatedAt">
						<CalendarIcon aria-hidden="true" />
						<div class="flex flex-col">
							<span>
								{{ formatMessage(messages.updatedLabel) }}
								{{ formatDate(project.updatedAt) }}
							</span>
						</div>
					</div>
					<div v-if="project?.license">
						<TagIcon aria-hidden="true" />
						<div class="flex flex-col">
							<span>{{ project.license }}</span>
						</div>
					</div>
				</div>
			</div>
		</div>
	</Teleport>
</template>

<script setup lang="ts">
import {
	CalendarIcon,
	CheckIcon,
	ClipboardCopyIcon,
	DownloadIcon,
	ExternalIcon,
	GlobeIcon,
	HeartIcon,
	LinkIcon,
	MoreVerticalIcon,
	SpinnerIcon,
	TagIcon,
	UserIcon,
} from '@modrinth/assets'
import {
	Button,
	Card,
	commonMessages,
	defineMessages,
	injectNotificationManager,
	NavTabs,
	ProjectPageHeader,
	TagItem,
	TeleportOverflowMenu,
	useFormatNumber,
	useVIntl,
} from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import dayjs from 'dayjs'
import { computed, onScopeDispose, watchEffect } from 'vue'
import { RouterView, useRoute, useRouter } from 'vue-router'

import {
	setSplitDetailColor,
	sourceProjectBasePathOf,
	useSplitView,
} from '@/composables/use-split-view.ts'
import { useSourceProject } from '@/composables/use-source-project'
import { sourceTintStyle } from '@/content-sources/catalog'
import { toError } from '@/helpers/errors'

const route = useRoute()
const router = useRouter()
const { formatMessage } = useVIntl()
const formatNumber = useFormatNumber()
const { handleError, addNotification } = injectNotificationManager()
const { splitViewActive } = useSplitView()

const {
	contentType,
	appearance,
	project,
	versions,
	provider,
	installed,
	installing,
	error,
	install,
} = useSourceProject()

const messages = defineMessages({
	loading: { id: 'app.source-project.loading', defaultMessage: 'Loading project…' },
	moreOptions: { id: 'app.source-project.more-options', defaultMessage: 'More options' },
	installedHint: {
		id: 'app.source-project.installed-hint',
		defaultMessage: 'Already installed in this instance',
	},
	openOnSource: {
		id: 'app.source-project.open-on-source',
		defaultMessage: 'Open on the source',
	},
	copyLink: { id: 'app.source-project.copy-link', defaultMessage: 'Copy link' },
	linkCopied: { id: 'app.source-project.link-copied', defaultMessage: 'Link copied' },
	linksTitle: { id: 'app.source-project.links', defaultMessage: 'Links' },
	tagsTitle: { id: 'app.source-project.tags', defaultMessage: 'Tags' },
	authorTitle: { id: 'app.source-project.author', defaultMessage: 'Author' },
	downloadsLabel: { id: 'app.source-project.downloads', defaultMessage: 'downloads' },
	followersLabel: { id: 'app.source-project.followers', defaultMessage: 'followers' },
	publishedLabel: { id: 'app.source-project.published', defaultMessage: 'Published' },
	updatedLabel: { id: 'app.source-project.updated', defaultMessage: 'Updated' },
	descriptionTab: { id: 'app.source-project.tab-description', defaultMessage: 'Description' },
	versionsTab: { id: 'app.source-project.tab-versions', defaultMessage: 'Versions' },
	galleryTab: { id: 'app.source-project.tab-gallery', defaultMessage: 'Gallery' },
})

/**
 * A Modrinth-shaped header, built from the source's project. The header is the
 * launcher's own component, so this is what makes the page look like every other
 * project page rather than like a second, parallel design.
 */
const headerProject = computed(() => ({
	id: project.value?.id ?? '',
	title: project.value?.name ?? '',
	description: project.value?.summary ?? '',
	status: 'approved' as const,
	downloads: project.value?.downloads ?? 0,
	followers: project.value?.follows ?? 0,
	categories: project.value?.tags ?? [],
	// With no project icon of its own, the source's mark stands in.
	icon_url: project.value?.iconUrl ?? appearance.value.iconUrl,
}))

const headerProjectV3 = computed(() => ({
	status: 'approved' as const,
	minecraft_server: null,
	minecraft_java_server: null,
}))

/**
 * The source's colour as the page's backdrop, fading from half opaque at the
 * top to nothing at the bottom. Only used at full width: in split view the same
 * colour is painted on the detail pane instead, since the backdrop layer is the
 * whole window and tinting the list beside it would say the wrong thing.
 */
const tintStyle = computed(() => sourceTintStyle(appearance.value.color))

/*
 * The same colour again, for the discover page's split pane. That pane is not
 * this component's to paint, so the colour is published for it to pick up —
 * and cleared on the way out so a pane left open does not keep the last
 * source's colour after its page is gone.
 */
watchEffect(() => setSplitDetailColor(appearance.value.color))
onScopeDispose(() => setSplitDetailColor(null))

const sourceLink = computed(() => project.value?.links?.[0]?.url)

const moreActions = computed(() => {
	const actions = []
	if (sourceLink.value) {
		actions.push({
			id: 'open_source',
			label: formatMessage(messages.openOnSource),
			icon: GlobeIcon,
			action: () => openLink(sourceLink.value as string),
		})
	}
	actions.push({
		id: 'copy_link',
		label: formatMessage(messages.copyLink),
		icon: ClipboardCopyIcon,
		action: () => {
			void navigator.clipboard.writeText(sourceLink.value ?? '').then(() =>
				addNotification({
					title: formatMessage(messages.linkCopied),
					type: 'success',
				}),
			)
		},
	})
	return actions
})

function openLink(url: string) {
	void openUrl(url)
}

function formatDate(iso: string): string {
	const parsed = dayjs(iso)
	return parsed.isValid() ? parsed.format('YYYY-MM-DD') : iso
}

const browsePath = computed(() => {
	const type = contentType.value
	return `/browse/${type}`
})

const basePath = computed(() => sourceProjectBasePathOf(route))

const tabLinks = computed(() => [
	{
		label: formatMessage(messages.descriptionTab),
		href: basePath.value,
	},
	{
		label: formatMessage(messages.versionsTab),
		href: `${basePath.value}/versions`,
		// Keeps the tab lit while one release's own page is open.
		subpages: ['/versions', '/version/'],
		shown: versions.value.length > 0,
	},
	{
		label: formatMessage(messages.galleryTab),
		href: `${basePath.value}/gallery`,
		shown: (project.value?.gallery?.length ?? 0) > 0,
	},
])

async function runInstall(options?: { versionId?: string; versionName?: string }) {
	try {
		await install(options)
	} catch (caught) {
		handleError(toError(caught))
	}
}
</script>

<style scoped lang="scss">
.project-sidebar-section {
	@apply flex flex-col gap-2 border-0 border-b-[1px] border-solid border-[--brand-gradient-border] p-4;
}

/* Split view: the same sections as a card at the end of the detail pane. */
.split-project-sidebar {
	@apply mx-6 mb-6 flex flex-col overflow-hidden rounded-2xl border-[1px] border-solid border-[--brand-gradient-border];

	> :last-child {
		@apply border-b-0;
	}
}
</style>
