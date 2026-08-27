<script lang="ts"></script>

<script setup lang="ts">
import { RightArrowIcon } from '@modrinth/assets'
import { type Component, type ComponentPublicInstance, computed, nextTick, ref } from 'vue'

import { type MessageDescriptor, useVIntl } from '../../composables/i18n'
import { useScrollIndicator } from '../../composables/scroll-indicator'
import { truncatedTooltip } from '../../utils/truncate'
import NewModal from './NewModal.vue'
export interface Tab {
	name: MessageDescriptor
	category?: MessageDescriptor
	icon: Component
	content?: Component
	href?: string
	badge?: MessageDescriptor
	shown?: boolean
}

const { formatMessage } = useVIntl()

const props = withDefaults(
	defineProps<{
		tabs: Tab[]
		header?: string
		maxWidth?: string
		width?: string
		closable?: boolean
		onHide?: () => void
		onShow?: () => void
		beforeHide?: () => boolean
		beforeTabChange?: (fromIndex: number, toIndex: number) => boolean
		floatingActionBarShown?: boolean
	}>(),
	{
		header: undefined,
		maxWidth: undefined,
		width: undefined,
		closable: true,
		onHide: undefined,
		onShow: undefined,
		beforeHide: undefined,
		beforeTabChange: undefined,
		floatingActionBarShown: false,
	},
)

const visibleTabs = computed(() => props.tabs.filter((tab) => tab.shown !== false))

const selectedTab = ref(0)
const tabLabelRefs = ref<Record<number, HTMLElement | null>>({})

function setTabLabelRef(index: number, element: Element | ComponentPublicInstance | null) {
	tabLabelRefs.value[index] = element instanceof HTMLElement ? element : null
}

function tabLabelTooltip(index: number, label: string) {
	return truncatedTooltip(tabLabelRefs.value[index], label)
}

const scrollContainer = ref<HTMLElement | null>(null)
const { showTopFade, showBottomFade, checkScrollState, forceCheck } =
	useScrollIndicator(scrollContainer)

const sidebarScrollContainer = ref<HTMLElement | null>(null)
const {
	showTopFade: showSidebarTopFade,
	showBottomFade: showSidebarBottomFade,
	checkScrollState: checkSidebarScrollState,
} = useScrollIndicator(sidebarScrollContainer)

const modal = ref<InstanceType<typeof NewModal> | null>(null)

// The fades used to be opaque `bg-raised → transparent` overlays painted on top
// of the content. That only looks right while the modal is opaque: with a
// translucent modal background the overlay is a visible solid smear. Masking the
// scroll container instead fades the content's own alpha out, so it works on any
// background, translucent or not.
const contentFadeStyle = computed(() => ({
	'--fade-top': showTopFade.value ? '1rem' : '0px',
	'--fade-bottom': showBottomFade.value ? '4rem' : '0px',
}))
const sidebarFadeStyle = computed(() => ({
	'--fade-top': showSidebarTopFade.value ? '1rem' : '0px',
	'--fade-bottom': showSidebarBottomFade.value ? '4rem' : '0px',
}))

function setTab(index: number) {
	if (index === selectedTab.value) return
	if (props.beforeTabChange?.(selectedTab.value, index) === false) return
	selectedTab.value = index
	nextTick(() => forceCheck())
}

function show(event?: MouseEvent) {
	modal.value?.show(event)
}

function hide(): boolean {
	return modal.value?.hide() ?? false
}

function startsCategory(index: number) {
	const category = visibleTabs.value[index]?.category
	return !!category && category.id !== visibleTabs.value[index - 1]?.category?.id
}

defineExpose({ show, hide, selectedTab, setTab })
</script>
<template>
	<NewModal
		ref="modal"
		:header="header"
		:max-width="maxWidth"
		:width="width"
		:closable="closable"
		:on-hide="onHide"
		:on-show="onShow"
		:before-hide="beforeHide"
		no-padding
	>
		<template v-if="$slots.title" #title>
			<slot name="title" />
		</template>
		<div class="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] p-6 pb-3 pr-0">
			<div
				class="flex min-w-0 max-h-[min(70vh,600px)] flex-col border-0 border-r-[1px] border-solid border-divider pr-4"
			>
				<div class="relative min-h-0 flex-1">
					<div
						ref="sidebarScrollContainer"
						class="scroll-fade flex h-full flex-col gap-1 overflow-y-auto"
						:style="sidebarFadeStyle"
						@scroll="checkSidebarScrollState"
					>
						<template v-for="(tab, index) in visibleTabs" :key="index">
							<div
								v-if="startsCategory(index) && tab.category"
								class="shrink-0 truncate px-4 pb-1 pt-2 text-xs font-bold uppercase tracking-wide text-secondary"
							>
								{{ formatMessage(tab.category) }}
							</div>
							<component
								:is="tab.href ? 'a' : 'button'"
								:href="tab.href ?? undefined"
								:target="tab.href ? '_blank' : undefined"
								:rel="tab.href ? 'noopener noreferrer' : undefined"
								:class="`flex shrink-0 min-w-0 gap-2 items-center text-left rounded-xl px-4 py-2 border-none font-semibold cursor-pointer active:scale-[0.97] transition-all no-underline ${!tab.href && selectedTab === index ? 'bg-button-bgSelected text-button-textSelected' : 'bg-transparent text-button-text hover:bg-button-bg hover:text-contrast'}`"
								@click="!tab.href && setTab(index)"
							>
								<component :is="tab.icon" class="w-4 h-4 flex-shrink-0" />
								<span
									:ref="(element) => setTabLabelRef(index, element)"
									v-tooltip="tabLabelTooltip(index, formatMessage(tab.name))"
									class="min-w-0 flex-1 truncate"
								>
									{{ formatMessage(tab.name) }}
								</span>
								<span
									v-if="tab.badge"
									class="shrink-0 rounded-full px-1.5 py-0.5 text-xs font-bold bg-brand-highlight text-brand-green"
								>
									{{ formatMessage(tab.badge) }}
								</span>
								<RightArrowIcon v-if="tab.href" class="ml-auto size-4 shrink-0" />
							</component>
						</template>
					</div>
				</div>

				<slot name="footer" />
			</div>
			<div class="relative min-h-[min(70vh,600px)]">
				<div
					ref="scrollContainer"
					class="scroll-fade absolute inset-0 overflow-y-auto px-6"
					:class="floatingActionBarShown ? 'pb-24' : 'pb-6'"
					:style="contentFadeStyle"
					@scroll="checkScrollState"
				>
					<Suspense>
						<component
							:is="visibleTabs[selectedTab]?.content"
							v-if="visibleTabs[selectedTab]?.content"
						/>
					</Suspense>
				</div>

				<div class="pointer-events-none absolute bottom-3 left-6 right-6 z-20">
					<div class="pointer-events-auto">
						<slot name="floating-action-bar" />
					</div>
				</div>
			</div>
		</div>
	</NewModal>
</template>

<style scoped>
/* Registering the two lengths lets the mask animate instead of snapping when a
   fade turns on or off. Browsers without @property support just get the
   instant switch. */
@property --fade-top {
	syntax: '<length>';
	inherits: false;
	initial-value: 0px;
}

@property --fade-bottom {
	syntax: '<length>';
	inherits: false;
	initial-value: 0px;
}

.scroll-fade {
	--fade-top: 0px;
	--fade-bottom: 0px;
	transition:
		--fade-top 200ms ease,
		--fade-bottom 200ms ease;
	-webkit-mask-image: linear-gradient(
		to bottom,
		transparent 0,
		#000 var(--fade-top),
		#000 calc(100% - var(--fade-bottom)),
		transparent 100%
	);
	mask-image: linear-gradient(
		to bottom,
		transparent 0,
		#000 var(--fade-top),
		#000 calc(100% - var(--fade-bottom)),
		transparent 100%
	);
	/* Default (non-`local`) attachment keeps the mask pinned to the element box,
	   so the fade stays at the top/bottom edges while the content scrolls. */
	-webkit-mask-repeat: no-repeat;
	mask-repeat: no-repeat;
	-webkit-mask-size: 100% 100%;
	mask-size: 100% 100%;
}
</style>
