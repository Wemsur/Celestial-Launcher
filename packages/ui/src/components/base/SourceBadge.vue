<script setup lang="ts">
/**
 * The mark for a content source: the source's own icon when it has one,
 * Modrinth's mark for the built-in source, and otherwise the source name's first
 * character on a disc of the source's colour.
 *
 * Lives here rather than in the app because both a card (this package) and the
 * app's source picker need it, and a source that looked different in the two
 * places would read as two different sources.
 */
import { ModrinthLogoIcon } from '@modrinth/assets'
import { computed } from 'vue'

const props = withDefaults(
	defineProps<{
		/** The source's display name; its first character is the fallback mark. */
		label: string
		/** `#rrggbb`. Absent means a neutral disc. */
		color?: string | null
		/** A displayable URL for the source's own icon. */
		iconUrl?: string | null
		/** Draw Modrinth's own mark instead of a letter. */
		isModrinth?: boolean
		/** Any CSS size; the badge is square. */
		size?: string
		/** Show the label as a tooltip. */
		tooltip?: boolean
	}>(),
	{ size: '20px', tooltip: false, color: null, iconUrl: null, isModrinth: false },
)

const DEFAULT_COLOR = '#8b8b8b'

const initial = computed(() => {
	const first = [...props.label.trim()][0] ?? '?'
	return first.toLocaleUpperCase()
})

/**
 * Black or white, whichever reads on the disc. Rec. 601 luma is close enough for
 * a single letter and needs no colour library.
 */
const initialTextColor = computed(() => {
	const hex = (props.color ?? DEFAULT_COLOR).replace('#', '')
	if (hex.length !== 6) return '#ffffff'
	const r = Number.parseInt(hex.slice(0, 2), 16)
	const g = Number.parseInt(hex.slice(2, 4), 16)
	const b = Number.parseInt(hex.slice(4, 6), 16)
	if ([r, g, b].some(Number.isNaN)) return '#ffffff'
	const luma = (0.299 * r + 0.587 * g + 0.114 * b) / 255
	return luma > 0.6 ? '#000000' : '#ffffff'
})

const box = computed(() => ({ width: props.size, height: props.size }))
const title = computed(() => (props.tooltip ? props.label : undefined))
</script>

<template>
	<span
		v-if="isModrinth"
		v-tooltip="title"
		class="inline-flex shrink-0 items-center justify-center"
		:style="{ ...box, color: color ?? DEFAULT_COLOR }"
	>
		<ModrinthLogoIcon class="h-full w-full" />
	</span>
	<img
		v-else-if="iconUrl"
		v-tooltip="title"
		:src="iconUrl"
		:alt="label"
		class="shrink-0 rounded-full object-cover"
		:style="box"
	/>
	<span
		v-else
		v-tooltip="title"
		class="inline-flex shrink-0 select-none items-center justify-center rounded-full font-semibold leading-none"
		:style="{
			...box,
			backgroundColor: color ?? DEFAULT_COLOR,
			color: initialTextColor,
			fontSize: `calc(${size} * 0.55)`,
		}"
	>
		{{ initial }}
	</span>
</template>
