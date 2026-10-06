<script setup lang="ts">
/**
 * The chip on a card saying which content source the item came from: the
 * source's mark plus its name, styled as one of the card's other tags so a
 * merged list reads uniformly.
 */
import SourceBadge from '../../base/SourceBadge.vue'
import TagItem from '../../base/TagItem.vue'

defineProps<{
	label: string
	color?: string | null
	iconUrl?: string | null
	isModrinth?: boolean
}>()
</script>

<template>
	<TagItem>
		<SourceBadge
			:label="label"
			:color="color"
			:icon-url="iconUrl"
			:is-modrinth="isModrinth"
			size="16px"
		/>
		<span class="truncate">{{ label }}</span>
	</TagItem>
</template>
