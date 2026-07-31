<script setup lang="ts">
import { NewModal as Modal, ButtonStyled, Checkbox } from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'

const emit = defineEmits<{
    proceed: []
    cancel: []
}>()

const modal = ref<InstanceType<typeof Modal> | null>(null)
const importing = ref(false)
const showRestartMessage = ref(false)
const dontShowAgain = ref(false)

async function doImport() {
    importing.value = true
    try {
        await invoke('import_old_data')
        // 导入成功！无论用户之前是否勾选，都标记为不再显示提示
        await setDontShowAgain(true)
        importing.value = false
        showRestartMessage.value = true
    } catch (err) {
        console.error('Import failed:', err)
        importing.value = false
    }
}

async function doRestart() {
    try {
        // 先保存设置，再执行导入+重启
        await setDontShowAgain(true)
        await invoke('do_import_and_restart')
    } catch (err) {
        console.error('Import and restart failed:', err)
    }
}

function handleCancel() {
    if (dontShowAgain.value) {
        setDontShowAgain(true)
    }
    emit('cancel')
    modal.value?.hide()
}

// 存入不再显示标记——复用现有的 save_settings 逻辑或直接写 JSON
async function setDontShowAgain(value: boolean) {
    try {
        await invoke('set_dont_show_import_modal', { value })
    } catch (err) {
        console.error('Failed to save don\'t show setting:', err)
    }
}

// 暴露 show/hide
defineExpose({
    show: () => {
        modal.value?.show()
    },
    hide: () => {
        modal.value?.hide()
    },
})
</script>

<template>
    <Modal ref="modal" :noblur="false" :closable="!importing" :hide-header="false">
        <template #title>
			<span class="font-extrabold text-contrast text-lg">
				{{ showRestartMessage ? '需要重启应用' : '导入旧数据' }}
			</span>
        </template>

        <div class="flex flex-col gap-4">
            <!-- 阶段 1: 提示导入 -->
            <p v-if="!importing && !showRestartMessage" class="m-0 max-w-[35rem]">
                检测到您之前使用过 Modrinth App 或旧版 Celetial Launcher。是否导入您的游戏实例、背景图片和设置？
            </p>
            <p v-if="!importing && !showRestartMessage" class="m-0 max-w-[35rem] text-red">
                这将覆盖现有的应用数据
            </p>
            <!-- 阶段 2: 转圈中 -->
            <div v-if="importing" class="flex flex-col items-center gap-4 py-4">
                <div class="spinner-border text-brand" style="width: 3rem; height: 3rem;" role="status">
                    <span class="visually-hidden">Loading...</span>
                </div>
                <p class="m-0 text-contrast">正在导入数据，请稍候...</p>
            </div>

            <!-- 阶段 3: 完成，提示重启 -->
            <p v-if="showRestartMessage" class="m-0 max-w-[35rem]">
                数据导入已完成。请点击关闭应用，稍后手动打开应用以生效。
            </p>

            <!-- 不再显示复选框（只在阶段 1 和 3 显示） -->
            <label v-if="!importing && !showRestartMessage" class="flex items-end gap-2 cursor-pointer select-none mt-2">
                <Checkbox v-model="dontShowAgain" label="不再显示此提示" class="!p-0" />
            </label>
            <!-- 按钮区 -->
            <div class="flex gap-2 justify-end">
                <!-- 阶段 1: 取消 + 导入 -->
                <ButtonStyled v-if="!importing && !showRestartMessage" @click="handleCancel">
                    <button class="!shadow-none">取消</button>
                </ButtonStyled>
                <ButtonStyled v-if="!importing && !showRestartMessage" color="brand">
                    <button @click="doImport">导入</button>
                </ButtonStyled>

                <!-- 阶段 3: 取消 + 重启 -->
                <ButtonStyled v-if="showRestartMessage" color="brand">
                    <button @click="doRestart">关闭应用</button>
                </ButtonStyled>
            </div>
        </div>
    </Modal>
</template>