<template>
	<div class="flex flex-col gap-4">
		<div class="flex flex-col gap-1">
			<h2 class="m-0 text-lg font-semibold text-contrast">
				{{ formatMessage(messages.nameLabel) }}
			</h2>
			<Input
				v-model="ctx.name"
				:placeholder="formatMessage(messages.namePlaceholder)"
				type="text"
				wrapper-class="w-full"
			/>
		</div>

		<h2 class="m-0 text-lg font-semibold text-contrast">
			{{ formatMessage(messages.pathLabel) }}
		</h2>
		<Input
			v-model="ctx.path"
			:placeholder="formatMessage(messages.pathPlaceholder)"
			:icon="BoxIcon"
			type="text"
			wrapper-class="w-full"
		>
			<template #right>
				<Button
					v-tooltip="formatMessage(messages.browse)"
					type="quiet"
					:aria-label="formatMessage(messages.browse)"
					class="ml-1.5"
					@click="ctx.browse()"
				>
					<FolderSearchIcon aria-hidden="true" />
				</Button>
			</template>
		</Input>

		<!-- The format was chosen on the previous stage; showing it again is what
		     lets the user notice a wrong pick without walking back to check. -->
		<div v-if="ctx.format" class="flex flex-col gap-1">
			<span class="text-sm font-medium text-primary">{{ formatMessage(messages.formatLabel) }}</span>
			<span class="text-sm text-secondary">
				{{
					formatMessage(
						ctx.format === 'minecraft'
							? messages.minecraftFormatTitle
							: messages.modrinthFormatTitle,
					)
				}}
			</span>
		</div>
	</div>
</template>

<script setup lang="ts">
import { BoxIcon, FolderSearchIcon } from '@modrinth/assets'

import { useVIntl } from '#ui/composables/i18n'

import { Button } from '../../../base/buttons'
import Input from '../../../base/inputs/Input.vue'
import { addLibraryMessages, injectAddLibraryContext } from '../add-library-context'

const ctx = injectAddLibraryContext()
const { formatMessage } = useVIntl()
const messages = addLibraryMessages
</script>
