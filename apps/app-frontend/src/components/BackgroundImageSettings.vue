<script setup lang="ts">
import { defineMessages, useVIntl } from '@modrinth/ui'
import { computed, onUnmounted,ref } from 'vue'

import {get_background_url, process_dragged_background} from '@/helpers/background'

const { formatMessage } = useVIntl()

const messages = defineMessages({
    bgSettingsTitle: {
        id: 'app.appearance-settings.background-image.title',
        defaultMessage: 'Background image',
    },
    bgSettingsDescription: {
        id: 'app.appearance-settings.background-image.description',
        defaultMessage: 'Customize the overall background of the Modrinth App',
    },
    dropZoneActive: {
        id: 'app.appearance-settings.background-image.drop-active',
        defaultMessage: 'Drop image here...',
    },
	bgSettingsHint: {
        id: 'app.appearance-settings.background-image.hint',
        defaultMessage: 'Drag and drop an image or click to customize your app background.',
    },
})

const fileInputRef = ref<HTMLInputElement | null>(null)
const previewUrl = ref<string>('')
const dragCounter = ref(0)
const isHighlighted = computed(() => dragCounter.value > 0)

const triggerFileInput = () => {
    fileInputRef.value?.click()
}



// 核心变更：改为 async 异步函数，执行二进制读取
const handleFile = async (file: File) => {
    if (!file.type.startsWith('image/')) {
        console.warn('[Validation] 拒绝非图片格式文件:', file.type)
        return
    }

    // 1. 生成前端本地临时预览
    if (previewUrl.value) {
        URL.revokeObjectURL(previewUrl.value)
    }
    previewUrl.value = URL.createObjectURL(file)

    try {
        console.log(`[Binary Process] 开始读取文件流: ${file.name}`)
        const arrayBuffer = await file.arrayBuffer()
        const uint8Array = new Uint8Array(arrayBuffer)

        console.log('[Binary Success] 准备向后端投递二进制数据...')

        // 1. 持久化存储到磁盘
        const savedPath = await process_dragged_background(uint8Array, file.name)
        console.log('[IPC Success] 后端已成功持久化背景图，保存路径为:', savedPath)
        document.body.classList.add('custom-bg-active');
        const bgUrl = await get_background_url()
        if (bgUrl) {
            document.documentElement.style.setProperty('--app-custom-background', `url('${bgUrl}')`)
        }
        // 为全局注入背景样式（确保带上 cover 居中与半透明，防止遮挡文字）
        if (!document.getElementById('custom-bg-runtime-style')) {
            const styleElement = document.createElement('style')
            styleElement.id = 'custom-bg-runtime-style'
            styleElement.innerHTML = `
				#app, .app-container, body {
					background-image: var(--app-custom-background) !important;
					background-size: cover !important;
					background-position: center !important;
					background-repeat: no-repeat !important;
					background-attachment: fixed !important;
				}
			`
            document.head.appendChild(styleElement)
        }

    } catch (error) {
        console.error('[IPC Error] 向后端投递或持久化失败:', error)
    }
}

const handleFileChange = (event: Event) => {
    const target = event.target as HTMLInputElement
    if (target.files && target.files.length > 0) {
        handleFile(target.files[0])
    }
}

const handleDragEnter = (event: DragEvent) => {
    event.preventDefault()
    dragCounter.value++
}

const handleDragOver = (event: DragEvent) => {
    event.preventDefault()
}

const handleDragLeave = (event: DragEvent) => {
    event.preventDefault()
    dragCounter.value--
}

const handleDrop = (event: DragEvent) => {
    event.preventDefault()
    dragCounter.value = 0
    const files = event.dataTransfer?.files
    if (files && files.length > 0) {
        handleFile(files[0])
    }
}

onUnmounted(() => {
    if (previewUrl.value) {
        URL.revokeObjectURL(previewUrl.value)
    }
})
</script>

<template>
    <div class="mt-6">
        <h2 class="m-0 text-lg font-semibold text-contrast">
            {{ formatMessage(messages.bgSettingsTitle) }}
        </h2>
        <p class="m-0 mt-1 mb-3">
            {{ formatMessage(messages.bgSettingsDescription) }}
        </p>

        <input
            ref="fileInputRef"
            type="file"
            class="hidden"
            accept="image/jpeg,image/png,image/webp"
            @change="handleFileChange"
        />

        <div
            class="relative flex h-36 w-full items-center justify-center overflow-hidden rounded-[20px] border border-dashed transition-[background,border-color,box-shadow] duration-200 focus-within:outline focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-brand border-surface-5 bg-surface-2 hover:bg-surface-3 aspect-[31/40] box-border"
            :class="[
				isHighlighted
					? 'border-brand-500 bg-black/40 text-brand-400'
					: 'border-neutral-700 bg-neutral-900/50 text-neutral-400 hover:border-neutral-500'
			]"
            @click="triggerFileInput"
            @dragenter="handleDragEnter"
            @dragover="handleDragOver"
            @dragleave="handleDragLeave"
            @drop="handleDrop"
        >
            <div v-if="previewUrl" class="absolute inset-0 z-0 opacity-20 pointer-events-none">
                <img :src="previewUrl" alt="Background Preview" class="h-full w-full object-cover" />
            </div>

            <div class="relative z-10 flex flex-col items-center gap-1 select-none">
                <p class="m-0 text-sm font-medium">
					<span v-if="isHighlighted" class="font-semibold">
						{{ formatMessage(messages.dropZoneActive) }}
					</span>
                    <span v-else-if="previewUrl" class="text-primary">
						已载入临时预览：点击或拖拽可更换新背景
					</span>
                    <span v-else class="text-base font-semibold leading-6">
						{{ formatMessage(messages.bgSettingsHint) }}
					</span>
                </p>
                <p class="m-0 text-xs text-neutral-500">
                    Supports JPG, PNG, WEBP
                </p>
            </div>

<!--			<template #button="{ open }">
					<DropdownIcon
						class="size-6 shrink-0 text-primary transition-transform duration-300"
						:class="{ 'rotate-180': open }"
					/>
					<span class="min-w-0 text-xl font-semibold leading-7 text-primary">
						{{ section.title }}
					</span>
					<Tooltip
						v-if="section.infoTooltip"
						theme="dismissable-prompt"
						placement="top"
						:triggers="['hover', 'focus']"
					>
						<span
							class="inline-flex size-6 shrink-0 items-center justify-center text-secondary transition-colors group-hover:text-primary"
							@click.stop
						>
							<UnknownIcon class="size-5" />
						</span>
						<template #popper>
							<p class="m-0 max-w-96 text-wrap text-sm font-medium leading-tight">
								{{ section.infoTooltip }}
							</p>
						</template>
					</Tooltip>
				</template>

				<Draggable
					v-if="section.kind === 'saved'"
					:list="draggableSavedSkins"
					class="grid w-full grid-cols-3 gap-3 min-[1300px]:grid-cols-4 min-[1750px]:grid-cols-5 min-[2050px]:grid-cols-6"
					:item-key="savedSkinKey"
					:disabled="readOnly || !canReorderSavedSkins"
					:animation="250"
					:swap-threshold="1"
					:invert-swap="false"
					:force-fallback="true"
					:fallback-on-body="true"
					:fallback-tolerance="4"
					ghost-class="skin-reorder-ghost"
					chosen-class="skin-reorder-chosen"
					drag-class="skin-reorder-drag"
					fallback-class="skin-reorder-fallback"
					@start="onSavedSkinDragStart"
					@end="onSavedSkinDragEnd"
				>
					<template #header>
						<SkinLikeTextButton
							ref="addSkinButton"
							class="aspect-[31/40] w-full min-w-0 box-border rounded-[20px]"
							dropzone
							:disabled="readOnly"
							:drag-active="!readOnly && isAddSkinButtonDragActive"
							@click="emit('add-skin')"
							@dragenter="emit('add-skin-dragenter', $event)"
							@dragover="emit('add-skin-dragover', $event)"
							@dragleave="emit('add-skin-dragleave', $event)"
							@drop="emit('add-skin-drop', $event)"
						>
							<template #icon>
								<PlusIcon class="size-8" />
							</template>
							{{ formatMessage(messages.addSkinButton) }}
							<template #subtitle>{{ formatMessage(messages.dragAndDropSubtitle) }}</template>
						</SkinLikeTextButton>
					</template>-->

        </div>
    </div>
</template>
