<template>
	<div class="flex flex-col gap-3">
		<BigOptionButton
			:icon="BoxesIcon"
			:title="formatMessage(messages.modrinthFormatTitle)"
			:description="formatMessage(messages.modrinthFormatDescription)"
			:selected="ctx.format === 'modrinth'"
			@click="choose('modrinth')"
		/>
		<BigOptionButton
			:icon="BoxIcon"
			:title="formatMessage(messages.minecraftFormatTitle)"
			:description="formatMessage(messages.minecraftFormatDescription)"
			:selected="ctx.format === 'minecraft'"
			@click="choose('minecraft')"
		/>
	</div>
</template>

<script setup lang="ts">
import { BoxesIcon, BoxIcon } from '@modrinth/assets'

import { useVIntl } from '#ui/composables/i18n'

import type { LibraryFormat } from '../../../../providers/library-management'
import BigOptionButton from '../../../base/BigOptionButton.vue'
import { addLibraryMessages, injectAddLibraryContext } from '../add-library-context'

const ctx = injectAddLibraryContext()
const { formatMessage } = useVIntl()
const messages = addLibraryMessages

function choose(format: LibraryFormat) {
	ctx.format = format
	// Choosing a format answers the only question this stage asks, so it moves
	// on immediately rather than making the user press a button to confirm it.
	ctx.advance()
}
</script>
