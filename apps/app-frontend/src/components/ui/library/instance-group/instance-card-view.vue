<script setup lang="ts">
import { Avatar, avatarTintBackground, truncatedTooltip } from '@modrinth/ui'
import { computed, ref } from 'vue'

import { useAppSettings } from '@/composables/use-app-settings.ts'
import { useTheme } from '@/composables/use-theme.ts'
import { getInstanceIconUrl } from '@/helpers/instance'
import type { GameInstance } from '@/helpers/types'

const props = withDefaults(
	defineProps<{
		instance: GameInstance
		selected?: boolean
	}>(),
	{
		selected: false,
	},
)

const iconSrc = computed(() => getInstanceIconUrl(props.instance.icon_path))
const appSettings = useAppSettings()
const theme = useTheme()
const compactMode = computed(() => appSettings.getFeatureFlag('compact_instance_cards'))

const nameRef = ref<HTMLElement | null>(null)
const versionRef = ref<HTMLElement | null>(null)

// ── Icon-derived card background ────────────────────────────────────────────
//
// Opt-in through its own toggle in Appearance settings, and tied to nothing else:
// the advanced-rendering switch does not gate it.
//
// `Avatar` is the authority on whether an icon exists: a path can point at a file
// that is gone, and it exposes `failed` for exactly that. Reading it keeps the
// card's backdrop and the avatar it sits behind from ever disagreeing.
const avatar = ref<InstanceType<typeof Avatar> | null>(null)
const iconLoaded = computed(() => !!iconSrc.value && !avatar.value?.failed)

const decorateBackground = computed(() => theme.instanceCardIconBg)
const showIconBackdrop = computed(() => decorateBackground.value && iconLoaded.value)

/**
 * Exactly the background an icon-less avatar paints, tint weight included, so a
 * card without an icon reads as one flat extension of its own placeholder.
 *
 * Wrapped in a second mix to make it translucent — alpha on the colour rather than
 * `opacity`, which would fade the text too. Nested `color-mix()` is valid CSS.
 */
const tintBackground = computed(
	() => `color-mix(in srgb, ${avatarTintBackground(props.instance.id)} 90%, transparent)`,
)
</script>

<template>
	<div
		class="relative isolate flex w-full min-w-0 select-none overflow-clip border border-solid text-left transition-all"
		:class="{
			'flex-row items-center justify-start gap-2.5 rounded-xl p-2.5': compactMode,
			'flex-col items-start justify-end gap-3 rounded-[20px] p-3': !compactMode,
			'[border-color:color-mix(in_srgb,var(--color-text-primary)_40%,transparent)] brightness-110':
				selected,
			// The card's resting border colour. It lives here rather than on the
			// wrapper in `instance-card.vue` because this component already sets a
			// `border-color`, and two utilities setting the same property from two
			// components resolve by stylesheet order — not by which class was written
			// last. Keeping both states in one place removes the race entirely.
			'border-brand-highlight': !selected,
			// Opaque as before unless the effect is on; with it on, the colour comes
			// from `tintBackground` below or from the two backdrop layers.
			'bg-surface-3': !decorateBackground,
			'bg-transparent': decorateBackground,
		}"
		:style="
			decorateBackground && !showIconBackdrop ? { backgroundColor: tintBackground } : undefined
		"
	>
		<!--
			The two decorative layers are grouped so a single `opacity` makes the whole
			backdrop translucent. It has to live on the group, not on the scrim: the icon
			image underneath the scrim is opaque, so thinning only the scrim let more of
			the icon through but never the page background — which is why the card still
			looked solid. `opacity` is safe here because this group holds no text; the
			name and version are outside it.
		-->
		<div
			v-if="showIconBackdrop"
			aria-hidden="true"
			class="pointer-events-none absolute inset-0 -z-10 opacity-80"
		>
			<!--
				Overhangs the card by 20% a side: `filter: blur()` samples transparent
				pixels past the element's edge, so an `inset-0` layer would ring the card
				with a soft halo. `overflow-clip` on the root crops the overspill.
				Quoted `url()` because the path is a converted file src and may hold
				characters bare `url()` would choke on.
			-->
			<div
				class="absolute -inset-[20%] bg-cover bg-center blur-[10px]"
				:style="{ backgroundImage: `url(&quot;${iconSrc}&quot;)` }"
			></div>
			<!-- Scrim in the card's own colour, so the name and version stay readable
			     over whatever the icon happens to be. -->
			<div
				class="absolute inset-0 [background-color:color-mix(in_srgb,var(--surface-3)_70%,transparent)]"
			></div>
		</div>
		<div
			class="relative flex shrink-0 items-center overflow-clip"
			:class="compactMode ? 'size-10 rounded-lg' : 'aspect-square min-w-full rounded-2xl'"
		>
			<Avatar
				ref="avatar"
				class="pointer-events-none outline-none"
				:class="compactMode ? '!rounded-lg' : '!rounded-2xl'"
				size="100%"
				:src="iconSrc"
				:tint-by="instance.id"
				alt=""
				no-shadow
			/>
			<slot name="loading" :compact="compactMode" />
			<div
				class="absolute z-[1] flex items-center justify-center"
				:class="compactMode ? 'inset-0' : 'bottom-1.5 right-1.5 size-12'"
			>
				<slot name="leading" :compact="compactMode" />
			</div>
		</div>
		<div
			class="flex min-w-0 w-full flex-col items-start justify-center gap-1 px-0.5"
			:class="{ 'pr-10': compactMode }"
		>
			<p
				ref="nameRef"
				v-tooltip="truncatedTooltip(nameRef, instance.name)"
				class="m-0 w-full truncate text-base font-semibold leading-5 text-contrast"
			>
				{{ instance.name }}
			</p>
			<p
				ref="versionRef"
				v-tooltip="truncatedTooltip(versionRef, `${instance.loader} ${instance.game_version}`)"
				class="m-0 w-full truncate text-sm font-medium capitalize leading-[18px] text-primary"
			>
				{{ instance.loader }} {{ instance.game_version }}
			</p>
		</div>
		<slot name="overlay" :compact="compactMode" />
	</div>
</template>
