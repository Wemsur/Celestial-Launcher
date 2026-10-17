<script setup lang="ts">
// 两个商店共用的 GitHub 加速设置：一个开关 + 一个镜像下拉。
//
// 抽成组件是因为插件商店和内容源商店各有一个「商店」标签页，两处都要有同一份
// 设置，而设置只有一份（存在 interface/ui-preferences.json），所以界面也共用，
// 避免两处各写一遍后慢慢漂移。
import { RefreshCwIcon } from '@modrinth/assets'
import { Button, DropdownSelect, Toggle } from '@modrinth/ui'
import { computed } from 'vue'

import { setUiPreferences, useUiPreferences } from '@/composables/use-ui-preferences'
import { GITHUB_MIRRORS, mirrorById } from '@/helpers/github-mirror'

const emit = defineEmits<{ refresh: [] }>()

const preferences = useUiPreferences()

const mirrorIds = GITHUB_MIRRORS.map((mirror) => mirror.id)

function mirrorLabel(id: string): string {
	return mirrorById(id)?.label ?? id
}

const mirrorDescription = computed(() => {
	const mirror = mirrorById(preferences.githubMirrorId)
	if (!mirror) return ''
	return `https://${mirror.id}/ 加在 GitHub 地址前面`
})

async function setEnabled(enabled: boolean) {
	await setUiPreferences({ githubMirrorEnabled: enabled })
	emit('refresh')
}

async function setMirror(id: string) {
	await setUiPreferences({ githubMirrorId: id })
	emit('refresh')
}
</script>

<template>
	<div
		class="mt-4 flex flex-col gap-3 rounded-xl border border-solid border-surface-5 bg-surface-3 p-4"
	>
		<div class="flex items-center justify-between gap-4">
			<div class="flex flex-col gap-1 min-w-0">
				<span class="text-sm font-semibold text-contrast">GitHub 加速镜像</span>
				<span class="text-xs text-secondary">
					商店索引和下载都改走镜像站。直连 GitHub 超时时开启；镜像站是第三方，默认关闭。
				</span>
			</div>
			<span class="inline-flex shrink-0">
				<Toggle
					id="store-github-mirror"
					:model-value="preferences.githubMirrorEnabled"
					@update:model-value="setEnabled"
				/>
			</span>
		</div>

		<div
			v-if="preferences.githubMirrorEnabled"
			class="flex items-center justify-between gap-4 border-0 border-t border-solid border-surface-5 pt-3"
		>
			<div class="flex flex-col gap-1 min-w-0">
				<span class="text-sm font-medium text-contrast">镜像站</span>
				<span class="text-xs text-secondary">{{ mirrorDescription }}</span>
			</div>
			<div class="flex shrink-0 items-center gap-2">
				<DropdownSelect
					:model-value="preferences.githubMirrorId"
					:options="mirrorIds"
					:display-name="mirrorLabel"
					name="GitHub mirror"
					@update:model-value="setMirror"
				/>
				<Button size="sm" type="outlined" title="重新加载商店" @click="emit('refresh')">
					<RefreshCwIcon />
				</Button>
			</div>
		</div>
	</div>
</template>
