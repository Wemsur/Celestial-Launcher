<script setup lang="ts">
import {
    AppearanceSettingsLayout,
    injectAuth,
    injectUserPreferences,
    provideAppearanceSettings,
    useSavable,
} from '@modrinth/ui'

import { computed, inject, onBeforeUnmount, onMounted, ref, watch } from 'vue'

import { type ColorTheme, useTheme, FeatureFlag } from '@/composables/use-theme.ts'
import { type AppSettings, get, set } from '@/helpers/settings.ts'
import { getOS } from '@/helpers/utils'
import { appSettingsModalContextKey } from '@/providers/app-settings-modal'
import {invoke} from "@tauri-apps/api/core";

import {TrashIcon} from "@modrinth/assets";

import BackgroundImageSettings from '@/components/BackgroundImageSettings.vue'



const theme = useTheme()
const auth = injectAuth()
const { updatePreferences } = injectUserPreferences()
const settingsModal = inject(appSettingsModalContextKey, null)
const os = await getOS()
const settings = ref(await get())

type AppearanceSettingsState = {
    theme: ColorTheme
    syncAcrossDevices: boolean
    advancedRendering: boolean
    nativeDecorations: boolean
}

function getAppearanceSettingsState(settings: AppSettings): AppearanceSettingsState {
    return {
        theme: settings.theme,
        syncAcrossDevices: settings.sync_theme_across_devices,
        advancedRendering: settings.advanced_rendering,
        nativeDecorations: settings.native_decorations,
    }
}

const { saved, current, changes, saving, hasChanges, reset, save } = useSavable(
    () => getAppearanceSettingsState(settings.value),
    async (appearanceChanges) => {
        const value = current.value

        const nextSettings: AppSettings = {
            ...settings.value,
            theme: value.theme,
            sync_theme_across_devices: value.syncAcrossDevices,
            advanced_rendering: value.advancedRendering,
            native_decorations: value.nativeDecorations,
        }

        await set(nextSettings)
        settings.value = nextSettings
        theme.preferred = value.theme
        theme.syncAcrossDevices = value.syncAcrossDevices
        theme.advancedRendering = value.advancedRendering
    },
)

const themeOptions = computed(() =>
    theme.options.filter(
        (option) =>
            option !== 'retro' || settings.value.developer_mode || current.value.theme === 'retro',
    ),
)

function setTheme(value: ColorTheme): void {
    current.value.theme = value
}

function setSyncAcrossDevices(enabled: boolean): void {
    current.value.syncAcrossDevices = enabled
}

function setAdvancedRendering(enabled: boolean): void {
    current.value.advancedRendering = enabled
}

function setNativeDecorations(enabled: boolean): void {
    current.value.nativeDecorations = enabled
}

watch(
    [() => current.value.theme, () => saved.value.theme],
    ([selectedTheme, savedTheme]) => {
        theme.preview = selectedTheme === savedTheme ? null : selectedTheme
    },
    { immediate: true },
)

async function saveAppearanceSettings(): Promise<void> {
    try {
        await save()
    } catch {
        return
    }
}

onMounted(() => {
    settingsModal?.registerUnsavedChangesController({
        hasChanges: () => hasChanges.value,
        getOriginal: () => saved.value,
        getModified: () => changes.value,
        isSaving: () => saving.value,
        reset,
        save: saveAppearanceSettings,
    })
})

onBeforeUnmount(() => {
    theme.preview = null
    settingsModal?.registerUnsavedChangesController(null)
})

provideAppearanceSettings({
    deferPersistence: true,
    theme: {
        current: computed(() => current.value.theme),
        options: themeOptions,
        system: computed(() => theme.native),
        set: setTheme,
        syncAcrossDevices: {
            value: computed(() => current.value.syncAcrossDevices),
            set: setSyncAcrossDevices,
        },
        syncDisabled: computed(() => !auth.user.value),
    },
    advancedRendering: {
        value: computed(() => current.value.advancedRendering),
        set: setAdvancedRendering,
    },
    nativeDecorations:
        os !== 'MacOS'
            ? {
                value: computed(() => current.value.nativeDecorations),
                set: setNativeDecorations,
            }
            : undefined,
    updatePreferences,
})
// 组件挂载时加载已保存的 hueValue
onMounted(async () => {
    await themeStore.loadHueValue()
})

const worldsInHomeFlag: FeatureFlag = 'worlds_in_home'
const skipNonEssentialWarningsFlag: FeatureFlag = 'skip_non_essential_warnings'
const skipUnknownPackWarningFlag: FeatureFlag = 'skip_unknown_pack_warning'
const showPlayTimeFlag: FeatureFlag = 'show_instance_play_time'
const hueValue = ref(0)

const delete_background = async() => {
    try {
        // 调用 Rust 后端删除文件
        await invoke('delete_background');

        // 3. 执行 CSS 清理 (根据我们之前的类名逻辑)
        document.body.classList.remove('custom-background-enabled');
        document.body.classList.remove('custom-bg-active');

        // 4. 执行 DOM 清理 (移除背景图片 DOM)
        const img = document.getElementById('custom-bg-layer');
        if (img) {
            img.remove();
        }
        console.log("背景已删除");
    } catch (e) {
        console.error("删除失败:", e);
    }
};

// 开启自定义背景模式
const enableCustomMode = () => {
    // 相当于 self.is_custom_mode = True
    document.body.classList.add('custom-bg-active');
};

// 关闭自定义背景模式（恢复默认）
const disableCustomMode = () => {
    // 相当于 self.is_custom_mode = False
    document.body.classList.remove('custom-bg-active');
};

// 存储自定义主题色
function onHueChange(event: Event) {
    const val = Number((event.target as HTMLInputElement).value)
    themeStore.saveHueValue(val)
}

//剔除light、dark主题
const filteredThemeOptions = computed(() =>
    themeStore.getThemeOptions().filter(t => !['light', 'dark'].includes(t))
)

</script>

<template>
    <AppearanceSettingsLayout />
</template>

<style lang="scss" scoped>
/* 轨道高度 */
input[type="range"] {
    &::-webkit-slider-runnable-track {
        height: 16px;
        border-radius: 9999px;

    }
    &::-moz-range-track {
        height: 6px;
        border-radius: 9999px;
    }

    /* 轨道背景色 = 渐变条 */
    &::-webkit-slider-runnable-track {
        background: linear-gradient(to right,
        hsl(0,100%,50%), hsl(60,100%,50%), hsl(120,100%,50%),
        hsl(180,100%,50%), hsl(240,100%,50%), hsl(300,100%,50%),
        hsl(360,100%,50%));
    }

    &::-moz-range-track {
        background: linear-gradient(to right,
        hsl(0,100%,50%), hsl(60,100%,50%), hsl(120,100%,50%),
        hsl(180,100%,50%), hsl(240,100%,50%), hsl(300,100%,50%),
        hsl(360,100%,50%));
    }

    /* Thumb */
    &::-webkit-slider-thumb {
        appearance: none;
        width: 18px;
        height: 18px;
        border-radius: 9999px;
        border: 2px solid #ffffff;
        box-shadow: 0 0 0 2px rgba(0,0,0,0.45);
        cursor: pointer;
    }

    &::-moz-range-thumb {
        appearance: none;
        width: 18px;
        height: 18px;
        border-radius: 9999px;
        border: 2px solid #ffffff;
        box-shadow: 0 0 0 2px rgba(0,0,0,0.45);
        cursor: pointer;
    }
}
</style>