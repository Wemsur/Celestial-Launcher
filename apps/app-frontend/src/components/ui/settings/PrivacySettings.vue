<script setup lang="ts">
import { Toggle } from '@modrinth/ui'
import { ref, watch } from 'vue'

import { optInAnalytics, optOutAnalytics } from '@/helpers/analytics'
import { get, set } from '@/helpers/settings.ts'

const settings = ref(await get())

watch(
	settings,
	async () => {
		if (settings.value.telemetry) {
			optInAnalytics()
		} else {
			optOutAnalytics()
		}

		await set(settings.value)
	},
	{ deep: true },
)
</script>

<template>
    <div class="mt-4 flex items-center justify-between gap-4">
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">遥测数据</h2>
            <p class="m-0 mt-1 text-sm">
                Modrinth 会收集匿名分析数据和使用数据，以改善我们的用户体验并为您提供个性化体验。
                禁用此选项后，您将选择退出，您的数据将不再被收集。
            </p>
        </div>
        <Toggle id="opt-out-analytics" v-model="settings.telemetry" />
    </div>

    <div class="mt-4 flex items-center justify-between gap-4">
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">Discord RPC</h2>
            <p class="m-0 mt-1 text-sm">
                管理 Discord Rich Presence 集成功能。禁用此功能后，您的 Discord 个人资料中将不再显示
                您正在使用“Modrinth”作为游戏或应用。
            </p>
            <p class="m-0 mt-2 text-sm">
                注意：这不会阻止任何特定实例的 Discord Rich Presence 集成功能，例如由模组添加的功能。
                （需要重启应用才能生效）
            </p>
        </div>
        <Toggle id="disable-discord-rpc" v-model="settings.discord_rpc" />
    </div>
</template>
