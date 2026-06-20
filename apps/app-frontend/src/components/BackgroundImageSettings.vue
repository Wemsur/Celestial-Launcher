<script setup lang="ts">
import { defineMessages, useVIntl } from '@modrinth/ui'
import { ref, computed, onUnmounted } from 'vue'
import {process_dragged_background, getAssetUrl, get_background_url} from '@/helpers/background'

const { formatMessage } = useVIntl()

const messages = defineMessages({
    bgSettingsTitle: {
        id: 'app.appearance-settings.background-image.title',
        defaultMessage: 'Background image',
    },
    bgSettingsDescription: {
        id: 'app.appearance-settings.background-image.description',
        defaultMessage: 'Drag and drop an image or click to customize your app background.',
    },
    dropZoneActive: {
        id: 'app.appearance-settings.background-image.drop-active',
        defaultMessage: 'Drop image here...',
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
            class="relative flex h-36 w-full items-center justify-center overflow-hidden rounded-lg border-2 border-dashed p-4 text-center transition-all duration-200 cursor-pointer"
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
					<span v-if="isHighlighted">
						{{ formatMessage(messages.dropZoneActive) }}
					</span>
                    <span v-else-if="previewUrl" class="text-brand-400">
						已载入临时预览：点击或拖拽可更换新背景
					</span>
                    <span v-else>
						{{ formatMessage(messages.bgSettingsDescription) }}
					</span>
                </p>
                <p class="m-0 text-xs text-neutral-500">
                    Supports JPG, PNG, WEBP
                </p>
            </div>
        </div>
    </div>
</template>