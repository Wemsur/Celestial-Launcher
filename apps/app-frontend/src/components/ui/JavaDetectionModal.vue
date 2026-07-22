<template>
	<ModalWrapper ref="detectJavaModal" header="Select java version" :show-ad-on-close="false">
		<div class="flex flex-col gap-4">
			<Table :columns="javaInstallColumns" :data="chosenInstallOptions" row-key="path">
				<template #cell-version="{ value }">
					<span class="font-semibold text-primary">{{ value }}</span>
				</template>
				<template #cell-path="{ value }">
					<span v-tooltip="value" class="block truncate font-mono text-xs">{{ value }}</span>
				</template>
				<template #cell-actions="{ row }">
					<div class="flex items-center justify-end">
						<ButtonStyled v-if="currentSelected.path === row.path">
							<button class="!shadow-none" disabled><CheckIcon /> 已选择</button>
						</ButtonStyled>
						<ButtonStyled v-else>
							<button class="!shadow-none" @click="setJavaInstall(row)"><PlusIcon /> 选择</button>
						</ButtonStyled>
					</div>
				</template>
				<template #empty-state>
					<div class="p-4 text-secondary">未找到 Java 安装！</div>
				</template>
			</Table>
			<div class="flex justify-end">
				<ButtonStyled type="outlined">
					<button
						class="!shadow-none !border-surface-4 !border"
						@click="$refs.detectJavaModal.hide()"
					>
						<XIcon />
						取消
					</button>
				</ButtonStyled>
			</div>
		</div>
	</ModalWrapper>
</template>
<script setup>
import { CheckIcon, PlusIcon, XIcon } from '@modrinth/assets'
import { ButtonStyled, injectNotificationManager, Table } from '@modrinth/ui'
import { ref } from 'vue'

import ModalWrapper from '@/components/ui/modal/ModalWrapper.vue'
import { trackEvent } from '@/helpers/analytics'
import { find_filtered_jres } from '@/helpers/jre.js'

const { handleError } = injectNotificationManager()

const chosenInstallOptions = ref([])
const detectJavaModal = ref(null)
const currentSelected = ref({})
const javaInstallColumns = [
	{ key: 'version', label: '版本', width: '9rem' },
	{ key: 'path', label: '路径' },
	{ key: 'actions', label: '状态', align: 'right', width: '10rem' },
]

defineExpose({
	show: async (version, currentSelectedJava) => {
		chosenInstallOptions.value = await find_filtered_jres(version).catch(handleError)

		currentSelected.value = currentSelectedJava
		if (!currentSelected.value) {
			currentSelected.value = { path: '', version: '' }
		}

		detectJavaModal.value.show()
	},
})

const emit = defineEmits(['submit'])

function setJavaInstall(javaInstall) {
	emit('submit', javaInstall)
	detectJavaModal.value.hide()
	trackEvent('JavaAutoDetect', {
		path: javaInstall.path,
		version: javaInstall.version,
	})
}
</script>
