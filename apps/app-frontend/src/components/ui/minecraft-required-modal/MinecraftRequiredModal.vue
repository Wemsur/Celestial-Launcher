<template>
	<NewModal ref="modal" :header="formatMessage(messages.header)" max-width="544px" no-padding>
		<div class="grid grid-cols-[1fr_auto] gap-2.5 h-[154px] px-7 pt-4 pb-1 pr-9">
			<div class="flex flex-col gap-2.5 items-start justify-center h-min mt-5">
				<div class="font-semibold text-xl text-contrast">
					{{ formatMessage(messages.descriptionHeader) }}
				</div>
				<div class="text-secondary leading-6">
					{{ formatMessage(messages.description) }}
				</div>
			</div>
			<div class="relative h-full w-[96px] overflow-hidden mx-3">
				<div class="absolute top-0 left-0 z-0 w-full flex grow-0 flex-col items-end p-0">
					<img :src="steveImage" alt="" class="self-stretch" />
				</div>
				<div
					class="absolute left-0 bottom-0 z-10 order-1 h-6 w-[120px] shrink-0 grow-0 bg-[linear-gradient(180deg,rgba(39,41,46,0)_0%,#27292E_80%,#27292E_100%)]"
				></div>
			</div>
		</div>

		<div class="flex flex-col gap-6 px-6 pb-6">
			<div class="grid grid-cols-2 gap-2">
				<Button
					type="colored"
					color="medal_promotion"
					:disabled="loadingSignIn"
					@click="offlineModalRef?.show()"
				>
					<LogInIcon />
					{{ formatMessage(messages.offlineSignIn) }}
				</Button>
				<Button type="colored" color="brand" :disabled="loadingSignIn" @click="signIn">
					<SpinnerIcon v-if="loadingSignIn" class="animate-spin" />
					<svg
						v-else
						width="20"
						height="20"
						viewBox="0 0 20 20"
						fill="none"
						xmlns="http://www.w3.org/2000/svg"
					>
						<rect width="9.25" height="9.25" fill="black" fill-opacity="0.9" />
						<rect x="10.75" width="9.25" height="9.25" fill="black" fill-opacity="0.9" />
						<rect y="10.75" width="9.25" height="9.25" fill="black" fill-opacity="0.9" />
						<rect x="10.75" y="10.75" width="9.25" height="9.25" fill="black" fill-opacity="0.9" />
					</svg>
					{{ formatMessage(messages.signIn) }}
				</Button>
			</div>
			<p class="m-0 text-center text-sm text-secondary">
				{{ formatMessage(messages.dontHaveAccount) }}
				<a
					class="text-blue font-medium hover:underline"
					href="https://www.minecraft.net/en-us/store/minecraft-java-bedrock-edition-pc"
				>
					{{ formatMessage(messages.getMinecraft) }}
				</a>
			</p>
		</div>
	</NewModal>

	<NewModal ref="offlineModalRef" header="添加离线账户" :max-width="'500px'">
		<form class="space-y-6 min-w-[400px]" @submit.prevent="handleCreateOffline">
			<label class="flex flex-col gap-2">
				<span class="font-semibold text-contrast">用户名</span>
				<StyledInput
					ref="offlineInputRef"
					v-model="offlineUsername"
					wrapper-class="w-full"
					placeholder="请输入玩家名..."
				/>
				<div v-if="offlineError" class="text-sm text-red">{{ offlineError }}</div>
			</label>
		</form>
		<template #actions>
			<div class="flex gap-2 justify-end">
				<Button type="outlined">
					<button @click="hideOfflineModal">
						<XIcon class="h-5 w-5" />
						取消
					</button>
				</Button>
				<Button type="colored" color="brand">
					<button :disabled="offlineSubmitting" @click="handleCreateOffline">
						<EditIcon class="h-5 w-5" />
						添加
					</button>
				</Button>
			</div>
		</template>
	</NewModal>
</template>

<script setup lang="ts">
import { EditIcon, LogInIcon, SpinnerIcon, XIcon } from '@modrinth/assets'
import {
	Button,
	defineMessages,
	injectNotificationManager,
	NewModal,
	StyledInput,
	useVIntl,
} from '@modrinth/ui'
import { inject, type Ref, ref } from 'vue'

import steveImage from '@/assets/steve-look-up-left.webp'
import type AccountsCard from '@/components/ui/AccountsCard.vue'
import { handleSevereError } from '@/composables/use-error.js'
import { trackEvent } from '@/helpers/analytics'
import {
	create_offline_user,
	login as loginFlow,
	set_default_user,
} from '@/helpers/auth.js'

const { formatMessage } = useVIntl()
const notificationManager = injectNotificationManager()
const accountsCard = inject('accountsCard') as Ref<InstanceType<typeof AccountsCard> | null>

const messages = defineMessages({
	header: {
		id: 'minecraft-required.header',
		defaultMessage: 'Minecraft required',
	},
	descriptionHeader: {
		id: 'minecraft-required.description-header',
		defaultMessage: 'Sign in to a Microsoft account',
	},
	description: {
		id: 'minecraft-required.description',
		defaultMessage:
			'You need a Microsoft account that owns Minecraft before you can launch and play.',
	},
	getSupport: {
		id: 'minecraft-required.get-support',
		defaultMessage: 'Get support',
	},
	offlineSignIn: {
		id: 'minecraft-required.offline-sign-in',
		defaultMessage: '离线登录',
	},
	signIn: {
		id: 'minecraft-required.sign-in',
		defaultMessage: 'Sign in to Microsoft',
	},
	dontHaveAccount: {
		id: 'minecraft-required.dont-have-account',
		defaultMessage: 'Don’t have an account?',
	},
	getMinecraft: {
		id: 'minecraft-required.get-minecraft',
		defaultMessage: 'Get Minecraft',
	},
})

const modal = ref<InstanceType<typeof NewModal>>()
const loadingSignIn = ref(false)

// 离线账户弹窗
const offlineModalRef = ref<InstanceType<typeof NewModal>>()
const offlineInputRef = ref<InstanceType<typeof StyledInput>>()
const offlineUsername = ref('')
const offlineSubmitting = ref(false)
const offlineError = ref('')

function show() {
	modal.value?.show()
}

async function signIn() {
	loadingSignIn.value = true

	try {
		const loggedIn = await loginFlow()
		if (!loggedIn) return

		await set_default_user(loggedIn.profile.id)
		await accountsCard.value?.refreshValues()
		await trackEvent('AccountLogIn', { source: 'MinecraftRequiredModal' })
		modal.value?.hide()
	} catch (error) {
		handleSevereError(error)
	} finally {
		loadingSignIn.value = false
	}
}

/** 校验用户名 */
function validateOfflineUsername(name: string): string {
	const trimmed = name.trim()
	if (!trimmed) return '用户名不能为空'
	if (!/^[A-Za-z0-9_]{3,16}$/.test(trimmed)) {
		return '用户名必须为3-16个字符（仅字母、数字和下划线）'
	}
	return ''
}

/** 创建离线账户 */
async function handleCreateOffline() {
	const error = validateOfflineUsername(offlineUsername.value)
	if (error) {
		offlineError.value = error
		return
	}

	offlineSubmitting.value = true
	offlineError.value = ''
	try {
		// 创建离线账户
		const newCred = await create_offline_user(offlineUsername.value.trim())
		// 设为默认激活账户
		await set_default_user(newCred.profile.id)
		// 刷新账户卡片
		await accountsCard.value?.refreshValues()
		await trackEvent('AccountLogIn', { source: 'MinecraftRequiredModal' })
		// 关闭弹窗
		hideOfflineModal()
		modal.value?.hide()
		notificationManager.addNotification({
			title: '成功',
			text: '离线账户创建成功',
			type: 'success',
		})
	} catch (err) {
		offlineError.value = err instanceof Error ? err.message : String(err)
	} finally {
		offlineSubmitting.value = false
	}
}

/** 关闭离线弹窗 */
function hideOfflineModal() {
	offlineModalRef.value?.hide()
	offlineUsername.value = ''
	offlineError.value = ''
}

defineExpose({
	show,
})
</script>
