<script setup lang="ts">
import {
    AppearanceSettingsLayout,
    injectAuth,
    injectUserPreferences,
    provideAppearanceSettings,
    Toggle,
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

const delete_background = async() => {
    try {
        await invoke('delete_background');

        document.body.classList.remove('custom-background-enabled');
        document.body.classList.remove('custom-bg-active');

        const img = document.getElementById('custom-bg-layer');
        if (img) {
            img.remove();
        }
        console.log("背景已删除");
    } catch (e) {
        console.error("删除失败:", e);
    }
};

</script>

<template>
    <AppearanceSettingsLayout>
        <!-- 色相条 -->
        <template #before-advanced>
            <section class="mt-8 border-0 border-t border-solid border-divider pt-6">
                <div>
                    <h2 class="m-0 text-lg font-semibold text-contrast">自定义颜色</h2>
                    <p class="m-0 mt-1">在支持自定义颜色的主题下自定义主题色</p>
                </div>
                <div class="relative mt-2 h-4 w-full select-none" style="height:10px">
                    <input
                        type="range"
                        min="0"
                        max="360"
                        :value="theme.hueValue"
                        class="h-5 w-full appearance-none rounded-full bg-transparent cursor-pointer focus:shadow-[0_0_0_4px_hsl(var(--brand-hue,217),91%,60%)] [&::-webkit-slider-runnable-track]:rounded-full [&::-moz-range-track]:rounded-full"
                        @input="theme.saveHueValue(Number(($event.target as HTMLInputElement).value))"
                    />
                </div>
            </section>

            <!-- 背景图片设置 -->
            <section class="mt-8 border-0 border-t border-solid border-divider pt-6">
                <BackgroundImageSettings />
                <button id="purge-cache" class="btn min-w-max m-2 mt-4" @click="delete_background">
                    <TrashIcon/>
                    清除已选择的背景
                </button>
            </section>

            <!-- 背景模糊开关 -->
            <section class="mt-8 border-0 border-t border-solid border-divider pt-6">
                <div class="flex items-center justify-between gap-4">
                    <div>
                        <h2 class="m-0 text-lg font-semibold text-contrast">背景模糊</h2>
                        <p class="m-0 mt-1">启用背景模糊效果（仅在设置了自定义背景时生效）</p>
                    </div>
                    <Toggle
                        id="custom-bg-blur"
                        :model-value="theme.customBgBlur"
                        @update:model-value="(v) => theme.toggleBgBlur(v)"
                    />
                </div>
            </section>

            <!-- 实例卡片图标底纹 -->
            <section class="mt-8 border-0 border-t border-solid border-divider pt-6">
                <div class="flex items-center justify-between gap-4">
                    <div>
                        <h2 class="m-0 text-lg font-semibold text-contrast">实例卡片图标背景</h2>
                        <p class="m-0 mt-1">
                            用实例图标的高度模糊版本作为主页的实例卡片背景。
                        </p>
                    </div>
                    <Toggle
                        id="instance-card-icon-bg"
                        :model-value="theme.instanceCardIconBg"
                        @update:model-value="(v) => theme.toggleInstanceCardIconBg(v)"
                    />
                </div>
            </section>
        </template>
    </AppearanceSettingsLayout>
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
