<script setup lang="ts">
import {
    ButtonStyled, Combobox, defineMessages, ThemeSelector, Toggle, useVIntl
} from '@modrinth/ui'
import { ref, watch, onMounted, computed} from 'vue'

import { get, set } from '@/helpers/settings.ts'
import { getOS } from '@/helpers/utils'
import { useTheming } from '@/store/state'
import type { ColorTheme, FeatureFlag } from '@/store/theme.ts'
import BackgroundImageSettings from '@/components/BackgroundImageSettings.vue'
import {invoke} from "@tauri-apps/api/core";
import {TrashIcon} from "@modrinth/assets";

const themeStore = useTheming()
const { formatMessage } = useVIntl()

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

const messages = defineMessages({
    colorThemeTitle: {
        id: 'app.appearance-settings.color-theme.title',
        defaultMessage: 'Color theme',
    },
    colorThemeDescription: {
        id: 'app.appearance-settings.color-theme.description',
        defaultMessage: 'Select your preferred color theme for Modrinth App.',
    },
    advancedRenderingTitle: {
        id: 'app.appearance-settings.advanced-rendering.title',
        defaultMessage: 'Advanced rendering',
    },
    advancedRenderingDescription: {
        id: 'app.appearance-settings.advanced-rendering.description',
        defaultMessage:
            'Enables advanced rendering such as blur effects that may cause performance issues without hardware-accelerated rendering.',
    },
    blurBackgroundTitle: {
        id: 'app.appearance-settings.blur-background.title',
        defaultMessage: 'BackgroundBlur',
    },
    blurBackgroundDescription: {
        id: 'app.appearance-settings.blur-background.description',
        defaultMessage:
            'Enables background blur when customizing background images.',
    },
    hideNametagTitle: {
        id: 'app.appearance-settings.hide-nametag.title',
        defaultMessage: 'Hide nametag',
    },
    hideNametagDescription: {
        id: 'app.appearance-settings.hide-nametag.description',
        defaultMessage: 'Disables the nametag above your player on the skins page.',
    },
    nativeDecorationsTitle: {
        id: 'app.appearance-settings.native-decorations.title',
        defaultMessage: 'Native decorations',
    },
    nativeDecorationsDescription: {
        id: 'app.appearance-settings.native-decorations.description',
        defaultMessage: 'Use system window frame (app restart required).',
    },
    minimizeLauncherTitle: {
        id: 'app.appearance-settings.minimize-launcher.title',
        defaultMessage: 'Minimize launcher',
    },
    minimizeLauncherDescription: {
        id: 'app.appearance-settings.minimize-launcher.description',
        defaultMessage: 'Minimize the launcher when a Minecraft process starts.',
    },
    defaultLandingPageTitle: {
        id: 'app.appearance-settings.default-landing-page.title',
        defaultMessage: 'Default landing page',
    },
    defaultLandingPageDescription: {
        id: 'app.appearance-settings.default-landing-page.description',
        defaultMessage: 'Change the page to which the launcher opens on.',
    },
    defaultLandingPageHome: {
        id: 'app.appearance-settings.default-landing-page.home',
        defaultMessage: 'Home',
    },
    defaultLandingPageLibrary: {
        id: 'app.appearance-settings.default-landing-page.library',
        defaultMessage: 'Library',
    },
    defaultLandingPageWorlds: {
        id: 'app.appearance-settings.default-landing-page.worlds',
        defaultMessage: 'Library',
    },
    selectOption: {
        id: 'app.appearance-settings.select-option',
        defaultMessage: 'Select an option',
    },
    jumpBackIntoWorldsTitle: {
        id: 'app.appearance-settings.jump-back-into-worlds.title',
        defaultMessage: 'Jump back into worlds',
    },
    jumpBackIntoWorldsDescription: {
        id: 'app.appearance-settings.jump-back-into-worlds.description',
        defaultMessage: 'Includes recent worlds in the "Jump back in" section on the Home page.',
    },
    toggleSidebarTitle: {
        id: 'app.appearance-settings.toggle-sidebar.title',
        defaultMessage: 'Toggle sidebar',
    },
    toggleSidebarDescription: {
        id: 'app.appearance-settings.toggle-sidebar.description',
        defaultMessage: 'Enables the ability to toggle the sidebar.',
    },
    unknownPackWarningTitle: {
        id: 'app.appearance-settings.unknown-pack-warning.title',
        defaultMessage: 'Warn me before installing unknown modpacks',
    },
    unknownPackWarningDescription: {
        id: 'app.appearance-settings.unknown-pack-warning.description',
        defaultMessage:
            "If you attempt to install a Modrinth Pack file (.mrpack) that isn't hosted on Modrinth, we'll make sure you understand the risks before installing it.",
    },
    skipNonEssentialWarningsTitle: {
        id: 'app.appearance-settings.skip-non-essential-warnings.title',
        defaultMessage: 'Skip non-essential warnings',
    },
    skipNonEssentialWarningsDescription: {
        id: 'app.appearance-settings.skip-non-essential-warnings.description',
        defaultMessage:
            'Automatically skips low-risk confirmations like duplicate modpack installs, normal content deletion, bulk updates, unlinking modpacks, and repair prompts. Dangerous warnings will still be shown.',
    },
    showPlayTimeTitle: {
        id: 'app.appearance-settings.show-play-time.title',
        defaultMessage: 'Show play time',
    },
    showPlayTimeDescription: {
        id: 'app.appearance-settings.show-play-time.description',
        defaultMessage: `Displays how much time you've spent playing an instance.`,
    },
})

const os = ref(await getOS())
const settings = ref(await get())

watch(
    settings,
    async() => {
        await set(settings.value)
    },
    { deep: true },
)
</script>
<template>
    <h2 class="m-0 text-lg font-semibold text-contrast">
        {{ formatMessage(messages.colorThemeTitle) }}
    </h2>
    <p class="m-0 mt-1">{{ formatMessage(messages.colorThemeDescription) }}</p>

    <ThemeSelector
        :update-color-theme="
        (theme: ColorTheme) => {
            themeStore.setThemeState(theme)
            settings.theme = theme
        }
    "
        :current-theme="settings.theme"
        :theme-options="filteredThemeOptions"
        system-theme-color="system"
    />
    <!-- 色相条 -->
    <div class="mt-4 mb-8">
        <h2 class="m-0 text-lg font-semibold text-contrast">自定义颜色</h2>
        <p class="m-0 mt-1">
            在支持自定义颜色的主题下自定义主题色
        </p>
        <div class="relative mt-2 h-4 w-full select-none" style="height:10px">
            <input
                type="range"
                min="0"
                max="360"
                :value="themeStore.hueValue"
                @input="onHueChange"
                class="h-5 w-full appearance-none rounded-full bg-transparent cursor-pointer focus:shadow-[0_0_0_4px_hsl(var(--brand-hue,217),91%,60%)] [&::-webkit-slider-runnable-track]:rounded-full [&::-moz-range-track]:rounded-full"
            />
        </div>
    </div>
    <BackgroundImageSettings/>
    <button id="purge-cache" class="btn min-w-max m-2" @click="delete_background">
        <TrashIcon/>
        清除已选择的背景
    </button>

    <div class="mt-6 flex items-center justify-between">
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">
                {{ formatMessage(messages.blurBackgroundTitle) }}
            </h2>
            <p class="m-0 mt-1">
                {{ formatMessage(messages.blurBackgroundDescription) }}
            </p>
        </div>
        <Toggle
            id="custom-bg-blur"
            :model-value="themeStore.customBgBlur"
            @update:model-value="
			(e) => {
				themeStore.toggleBgBlur(!!e)
			}
			"
        />
    </div>

    <div class="mt-6 flex items-center justify-between">
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">
                {{ formatMessage(messages.advancedRenderingTitle) }}
            </h2>
            <p class="m-0 mt-1">
                {{ formatMessage(messages.advancedRenderingDescription) }}
            </p>
        </div>
        <Toggle
            id="advanced-rendering"
            :model-value="themeStore.advancedRendering"
            @update:model-value="
				(e) => {
					themeStore.advancedRendering = !!e
					settings.advanced_rendering = themeStore.advancedRendering
				}
			"
        />
    </div>

    <div v-if="os !== 'MacOS'" class="mt-6 flex items-center justify-between gap-4">
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">
                {{ formatMessage(messages.nativeDecorationsTitle) }}
            </h2>
            <p class="m-0 mt-1">{{ formatMessage(messages.nativeDecorationsDescription) }}</p>
        </div>
        <Toggle id="native-decorations" v-model="settings.native_decorations"/>
    </div>
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