<template>
	<MultiStageModal
		ref="modal"
		:stages="stages"
		:context="ctx"
		disable-progress
		@hide="handleHide"
	/>
</template>

<script setup lang="ts">
import { markRaw, useTemplateRef } from 'vue'
import type { ComponentExposed } from 'vue-component-type-helpers'

import { injectNotificationManager } from '../../../providers'
import type { LibraryFormat } from '../../../providers/library-management'
import type { StageConfigInput } from '../../base/MultiStageModal.vue'
import MultiStageModal from '../../base/MultiStageModal.vue'
import AddLibraryDetailsStage from './AddLibraryDetailsStage.vue'
import AddLibraryFormatStage from './AddLibraryFormatStage.vue'
import {
	type AddLibraryContextValue,
	createAddLibraryContext,
	provideAddLibraryContext,
} from './add-library-context'

const emit = defineEmits<{
	(e: 'hide'): void
	(e: 'added', path: string): void
}>()

const { handleError } = injectNotificationManager()

const modal = useTemplateRef<ComponentExposed<typeof MultiStageModal>>('modal')
const ctx = createAddLibraryContext(modal)
provideAddLibraryContext(ctx)

const stages: StageConfigInput<AddLibraryContextValue>[] = [
	{
		id: 'format',
		title: (context) => context.formatMessage(context.messages.chooseFormatTitle),
		stageContent: markRaw(AddLibraryFormatStage),
		leftButtonConfig: null,
		rightButtonConfig: null,
		maxWidth: '560px',
	},
	{
		id: 'details',
		title: (context) => context.formatMessage(context.messages.detailsTitle),
		stageContent: markRaw(AddLibraryDetailsStage),
		leftButtonConfig: (context) => ({
			label: context.formatMessage(context.messages.back),
			onClick: () => modal.value?.setStage('format'),
		}),
		rightButtonConfig: (context) => ({
			label: context.formatMessage(context.messages.addButton),
			color: 'brand',
			disabled: !context.path.trim() || context.submitting,
			loading: context.submitting,
			onClick: () => void submit(),
		}),
		maxWidth: '560px',
	},
]

async function submit() {
	if (!ctx.path.trim() || ctx.submitting) return
	ctx.submitting = true
	try {
		const added = await ctx.addLibrary()
		modal.value?.hide()
		emit('added', added)
	} catch (error) {
		handleError(error as Error)
	} finally {
		ctx.submitting = false
	}
}

function handleHide() {
	ctx.reset()
	emit('hide')
}

/**
 * Open the dialog on its first stage, the format question.
 *
 * Every entry point — the home page's add-library button, the library page and
 * the creation flow's "create or import a library" option — leads here, so the
 * two flows cannot drift apart on wording or on which layouts exist.
 */
function show(format?: LibraryFormat) {
	ctx.reset()
	ctx.format = format ?? null
	modal.value?.setStage('format')
	modal.value?.show()
}

function hide() {
	modal.value?.hide()
}

defineExpose({ show, hide })
</script>
