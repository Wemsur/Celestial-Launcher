<template>
	<div
		v-if="accounts.length === 0"
		class="flex flex-col gap-3 bg-button-bg border border-solid border-surface-5 rounded-xl p-3 mt-2"
	>
		<span>{{ formatMessage(messages.notSignedIn) }}</span>
		<ButtonStyled color="brand">
			<button color="primary" :disabled="loginDisabled" @click="login()">
				<LogInIcon v-if="!loginDisabled" />
				<SpinnerIcon v-else class="animate-spin" />
				{{ formatMessage(messages.signInToMinecraft) }}
			</button>
		</ButtonStyled>
	</div>
	<Accordion
		v-else
		class="w-full mt-2 bg-button-bg border border-solid border-surface-5 rounded-xl overflow-clip"
		button-class="button-base w-full bg-transparent px-3 py-2 border-0 cursor-pointer"
		:open-by-default="false"
	>
        <template #title>
            <div class="flex gap-2 w-full min-w-0">
                <template v-if="avatarUrl">
                    <Avatar size="36px" :src="avatarUrl" />
                </template>
                <template v-else>
                    <span
                        v-if="selectedAccount"
                        class="inline-flex w-9 h-9 rounded items-center justify-center text-white font-bold text-sm shrink-0"
                        :style="{ backgroundColor: getOfflineAvatarColor(selectedAccount.profile.name) }"
                    >
                        {{ selectedAccount.profile.name.charAt(0).toUpperCase() }}
                    </span>
                    <img
                        v-else
                        src="https://launcher-files.modrinth.com/assets/steve_head.png"
                        class="w-9 h-9 rounded shrink-0"
                        alt=""
                    />
                </template>
                <div class="flex flex-col items-start w-full min-w-0">
            <span class="truncate w-full text-left">{{
                    selectedAccount ? selectedAccount.profile.name : formatMessage(messages.selectAccount)
                }}</span>
                    <span class="text-secondary text-xs">{{ getAccountTypeLabel() }}</span>
                </div>
            </div>
        </template>
		<div class="bg-button-bg pt-1 pb-2 border border-solid border-surface-5">
			<template v-if="accounts.length > 0">
				<div v-for="account in accounts" :key="account.profile.id" class="flex gap-1 items-center">
					<button
						class="flex items-center flex-shrink flex-grow overflow-clip gap-2 p-2 border-0 bg-transparent cursor-pointer button-base min-w-0"
						@click="setAccount(account)"
					>
						<RadioButtonCheckedIcon
							v-if="selectedAccount && selectedAccount.profile.id === account.profile.id"
							class="w-5 h-5 text-brand shrink-0"
						/>
						<RadioButtonIcon v-else class="w-5 h-5 text-secondary shrink-0" />
                        <span
                            v-if="account.access_token === 'OFFLINE'"
                            class="inline-flex w-6 h-6 rounded items-center justify-center text-white font-bold text-xs shrink-0"
                            :style="{ backgroundColor: getOfflineAvatarColor(account.profile.name) }"
                        >
                            {{ account.profile.name.charAt(0).toUpperCase() }}
                        </span>
                        <Avatar
                            v-else
                            :src="getAccountAvatarUrl(account)"
                            size="24px"
                        />
						<p
							class="m-0 truncate min-w-0"
							:class="
								selectedAccount && selectedAccount.profile.id === account.profile.id
									? 'text-contrast font-semibold'
									: 'text-primary'
							"
						>
							{{ account.profile.name }}
						</p>
					</button>
					<ButtonStyled circular color="red" color-fill="none" hover-color-fill="background">
						<button
							v-tooltip="formatMessage(messages.removeAccount)"
							class="mr-2"
							@click="logout(account.profile.id)"
						>
							<TrashIcon />
						</button>
					</ButtonStyled>
				</div>
			</template>
			<div class="flex flex-col gap-2 px-2 pt-2">
				<ButtonStyled v-if="accounts.length > 0" class="w-full">
					<button :disabled="loginDisabled" @click="login()">
						<PlusIcon />
						正版登录
					</button>
				</ButtonStyled>
                <ButtonStyled v-if="accounts.length > 0" class="w-full">
                    <button :disabled="loginDisabled" @click="offlineModalRef?.show()">
                        <PlusIcon />
                        离线登录
                    </button>
                </ButtonStyled>
			</div>
		</div>
	</Accordion>
    <NewModal
        ref="offlineModalRef"
        header="添加离线账户"
        :max-width="'500px'"
    >
        <form @submit.prevent="handleCreateOffline" class="space-y-6 min-w-[400px]">
            <label class="flex flex-col gap-2">
                <span class="font-semibold text-contrast">用户名</span>
                <StyledInput
                    ref="offlineInputRef"
                    v-model="offlineUsername"
                    wrapper-class="w-full"
                />
                <div v-if="offlineError" class="text-sm text-red">{{ offlineError }}</div>
            </label>
        </form>
        <template #actions>
            <div class="flex gap-2 justify-end">
                <ButtonStyled type="outlined">
                    <button @click="hideOfflineModal">
                        <XIcon class="h-5 w-5" />
                        Cancel
                    </button>
                </ButtonStyled>
                <ButtonStyled color="brand">
                    <button :disabled="offlineSubmitting" @click="handleCreateOffline">
                        <EditIcon class="h-5 w-5" />
                        Add
                    </button>
                </ButtonStyled>
            </div>
        </template>
    </NewModal>
</template>

<script setup lang="ts">
import {
	LogInIcon,
	PlusIcon,
	RadioButtonCheckedIcon,
	RadioButtonIcon,
	SpinnerIcon,
	TrashIcon,
} from '@modrinth/assets'
import {
	Accordion,
	Avatar,
	ButtonStyled,
    NewModal,
    StyledInput,
	defineMessages,
	injectNotificationManager,
	useVIntl,
} from '@modrinth/ui'
import type { Ref } from 'vue'
import { computed, onUnmounted, ref } from 'vue'

import { trackEvent } from '@/helpers/analytics'
import {
	get_default_user,
	login as login_flow,
	remove_user,
	set_default_user,
	users,
} from '@/helpers/auth'
import { process_listener } from '@/helpers/events'
import { generatePlayerHeadBlob } from '@/helpers/rendering/batch-skin-renderer.ts'
import { getPlayerHeadUrl } from '@/helpers/rendering/batch-skin-renderer.ts'
import type { Skin } from '@/helpers/skins'
import { get_available_skins } from '@/helpers/skins'
import { handleSevereError } from '@/store/error.js'

import { XIcon, EditIcon } from '@modrinth/assets'
import { create_offline_user } from '@/helpers/auth'

const { formatMessage } = useVIntl()
const notificationManager = injectNotificationManager()
const { handleError } = notificationManager

const emit = defineEmits<{
	change: []
}>()

type MinecraftCredential = {
	profile: {
		id: string
		name: string
	}
    access_token: string
    active: boolean
}

const accounts: Ref<MinecraftCredential[]> = ref([])
const loginDisabled = ref(false)
const defaultUser = ref<string | undefined>()
const equippedSkin = ref<Skin | null>(null)
const headUrlCache = ref(new Map<string, string>())

// 离线账户弹窗
const offlineModalRef = ref<InstanceType<typeof NewModal>>()
const offlineInputRef = ref<InstanceType<typeof StyledInput>>()
const offlineUsername = ref('')
const offlineSubmitting = ref(false)
const offlineError = ref('')


async function refreshValues() {
    defaultUser.value = await get_default_user().catch(handleError)
    const userList = await users().catch(handleError)
    accounts.value = Array.isArray(userList) ? [...userList] : []
    accounts.value.sort((a, b) => {
        // 1. 活跃账户排最前
        if (a.active && !b.active) return -1
        if (!a.active && b.active) return 1

        // 2. 正版账户在前，离线账户在后
        const aIsOnline = a.access_token !== 'OFFLINE'
        const bIsOnline = b.access_token !== 'OFFLINE'
        if (aIsOnline && !bIsOnline) return -1
        if (!aIsOnline && bIsOnline) return 1

        // 3. 同组内按名字字母排序
        return (a.profile?.name ?? '').localeCompare(b.profile?.name ?? '')
    })

    try {
        const skins = await get_available_skins()
        equippedSkin.value = skins.find((skin) => skin.is_equipped) ?? null

        if (equippedSkin.value) {
            try {
                const headUrl = await getPlayerHeadUrl(equippedSkin.value)
                headUrlCache.value = new Map(headUrlCache.value).set(
                    equippedSkin.value.texture_key,
                    headUrl,
                )
            } catch (error) {
                console.warn('Failed to get head render for equipped skin:', error)
            }
        }
    } catch {
        equippedSkin.value = null
    }
}

async function setEquippedSkin(skin: Skin) {
	equippedSkin.value = skin

	try {
		const headUrl = await getPlayerHeadUrl(skin)
		headUrlCache.value = new Map(headUrlCache.value).set(skin.texture_key, headUrl)
	} catch (error) {
		console.warn('Failed to get head render for equipped skin:', error)
	}
}

function setLoginDisabled(value: boolean) {
	loginDisabled.value = value
}

defineExpose({
	refreshValues,
	setEquippedSkin,
	setLoginDisabled,
	loginDisabled,
})

await refreshValues()

const selectedAccount = computed(() =>
	accounts.value.find((account) => account.profile.id === defaultUser.value),
)

const avatarUrl = computed(() => {
    if (!selectedAccount.value) {
        return 'https://launcher-files.modrinth.com/assets/steve_head.png'
    }

    // 离线账户：不返回 mc-heads URL，模板里用 CSS 头像替代
    if (selectedAccount.value.access_token === 'OFFLINE') {
        return null
    }

    // 如果有装备的皮肤，优先用缓存
    if (equippedSkin.value?.texture_key) {
        const cachedUrl = headUrlCache.value.get(equippedSkin.value.texture_key)
        if (cachedUrl) {
            return cachedUrl
        }
    }

    // 尝试从账户列表中获取该账户的皮肤 URL 并渲染头像
    const skinUrl = selectedAccount.value.profile?.skins?.[0]?.url
    if (skinUrl) {
        const headKey = `head-${selectedAccount.value.profile.id}`
        if (!headUrlCache.value.has(headKey)) {
            generatePlayerHeadBlob(skinUrl, 128).then(blob => {
                const url = URL.createObjectURL(blob)
                headUrlCache.value = new Map(headUrlCache.value).set(headKey, url)
            }).catch(() => {})
        }
        const cached = headUrlCache.value.get(headKey)
        if (cached) return cached
    }
    // 所有途径都失败，回退到 Steve 默认头像
    return 'https://launcher-files.modrinth.com/assets/steve_head.png'
})

function getAccountAvatarUrl(account: MinecraftCredential) {
    // 离线账户：返回 null，模板用 CSS 头像
    if (account.access_token === 'OFFLINE') {
        return null
    }

    // 从账户的 profile.skins 获取皮肤 URL 并渲染头像
    const skinUrl = account.profile?.skins?.[0]?.url
    if (skinUrl) {
        const headKey = `head-${account.profile.id}`
        if (!headUrlCache.value.has(headKey)) {
            generatePlayerHeadBlob(skinUrl, 24).then(blob => {
                const url = URL.createObjectURL(blob)
                headUrlCache.value = new Map(headUrlCache.value).set(headKey, url)
            }).catch(() => {})
        }
        const cached = headUrlCache.value.get(headKey)
        if (cached) return cached
    }
    return null
}

function getAccountTypeLabel() {
    if (!selectedAccount.value) {
        return formatMessage(messages.minecraftAccount)
    }
    return selectedAccount.value.access_token === 'OFFLINE'
        ? '离线账户'
        : '正版账户'
}

function getOfflineAvatarColor(name: string) {
    let hash = 0
    for (let i = 0; i < name.length; i++) {
        hash = name.charCodeAt(i) + ((hash << 5) - hash)
    }
    var hue = Math.abs(hash % 360)
    return 'hsl(' + hue + ', 60%, 45%)'
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
        // 刷新列表（包含头像渲染）
        await refreshValues()
        // 关闭弹窗
        hideOfflineModal()
        // 通知
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

/** 关闭弹窗 */
function hideOfflineModal() {
    offlineModalRef.value?.hide()
    offlineUsername.value = ''
    offlineError.value = ''
}

async function setAccount(account: MinecraftCredential) {
	defaultUser.value = account.profile.id
	await set_default_user(account.profile.id).catch(handleError)
	await refreshValues()
	emit('change')
}

async function login() {
	loginDisabled.value = true
	const loggedIn = await login_flow().catch(handleSevereError)

	if (loggedIn) {
		await setAccount(loggedIn)
	}

	trackEvent('AccountLogIn')
	loginDisabled.value = false
}

async function logout(id: string) {
	await remove_user(id).catch(handleError)
	await refreshValues()
	if (!selectedAccount.value && accounts.value.length > 0) {
		await setAccount(accounts.value[0])
	} else {
		emit('change')
	}
	trackEvent('AccountLogOut')
}

const unlisten = await process_listener(async (e: { event: string }) => {
	if (e.event === 'launched') {
		await refreshValues()
	}
})

onUnmounted(() => {
	unlisten()
})

const messages = defineMessages({
	notSignedIn: {
		id: 'minecraft-account.not-signed-in',
		defaultMessage: 'Not signed in',
	},
	addAccount: {
		id: 'minecraft-account.add-account',
		defaultMessage: 'Add account',
	},
	removeAccount: {
		id: 'minecraft-account.remove-account',
		defaultMessage: 'Remove account',
	},
	selectAccount: {
		id: 'minecraft-account.select-account',
		defaultMessage: 'Select account',
	},
	minecraftAccount: {
		id: 'minecraft-account.label',
		defaultMessage: 'Minecraft account',
	},
	signInToMinecraft: {
		id: 'minecraft-account.sign-in',
		defaultMessage: 'Sign in to Minecraft',
	},
})
</script>
