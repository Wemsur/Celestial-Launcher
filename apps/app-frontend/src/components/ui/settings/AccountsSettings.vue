<template>
    <div class="flex flex-col gap-6">

        <!-- 第一部分：创建离线账户 -->
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">Offline Account</h2>
            <p class="m-0 mt-1 text-sm text-secondary">
                Create a local Minecraft account that works without internet.
                Username must be 3-16 characters (letters, numbers, underscore).
            </p>

            <div class="mt-4 flex gap-2">
                <StyledInput
                    :model-value="newOfflineUsername"
                    @update:model-value="newOfflineUsername = $event"
                    placeholder="Enter username..."
                    input-class="flex-1"
                    :disabled="creatingOffline"
                    clearable
                    @keyup.enter="onCreateOffline"
                />
                <button
                    class="btn btn-brand"
                    @click="onCreateOffline"
                    :disabled="!newOfflineUsername.trim()"
                >
                    Create
                </button>
            </div>
        </div>

        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">Microsoft Login</h2>
            <button
                    class="btn btn-brand"
                    @click="handleMinecraftLogin"
                >
                添加正版账户
            </button>
        </div>

        <hr class="bg-button-border border-none h-[1px]" />

        <!-- 第二部分：Minecraft 账户列表 -->
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">Minecraft Accounts</h2>

            <div v-if="loading" class="mt-4 text-secondary">Loading...</div>

            <div v-else-if="minecraftUsers.length === 0" class="mt-4">
                <EmptyState type="empty" heading="No accounts" description="Add an offline account or sign in with Microsoft." />
            </div>

            <div v-else class="mt-4 flex flex-col gap-3">
                <div
                    v-for="user in minecraftUsers"
                    :key="user.profile.id"
                    class="flex items-center justify-between rounded-lg bg-button-bg p-3"
                    :class="{ 'ring-2 ring-brand': user.active }"
                >
                    <div class="flex items-center gap-3">
                        <!-- 头像：离线账户用 CSS 首字母，正版账户用皮肤渲染头像 -->
                        <div
                            v-if="user.access_token === 'OFFLINE'"
                            class="w-10 h-10 rounded flex items-center justify-center text-white font-bold text-sm shrink-0"
                            :style="{ backgroundColor: getOfflineAvatarColor(user.profile.name) }"
                        >
                            {{ getInitial(user.profile.name) }}
                        </div>
                        <img
                            v-else
                            :src="getUserHeadUrl(user)"
                            class="w-10 h-10 rounded shrink-0"
                            alt=""
                        />
                        <div>
            <p class="font-semibold">{{ user.profile.name }}</p>
                            <p class="text-xs text-secondary">{{ user.profile.id }}</p>
                            <p class="text-xs" :class="user.access_token === 'OFFLINE' ? 'text-orange' : 'text-brand'">
                                {{ user.access_token === 'OFFLINE' ? 'Offline' : 'Online' }}
                            </p>
                        </div>
                    </div>

                    <div class="flex gap-2">
                        <button
                            v-if="!user.active"
                            class="btn btn-standard btn-small"
                            @click="onSetDefault(user.profile.id)"
                        >
                            Set Default
                        </button>
                        <button
                            class="btn btn-standard btn-small btn-red"
                            @click="onRemoveUser(user.profile.id)"
                        >
                            Remove
                        </button>
                    </div>
                </div>
            </div>
        </div>

        <hr class="bg-button-border border-none h-[1px]" />

        <!-- 第三部分：Modrinth 账户 -->
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">Modrinth Account</h2>
            <p class="m-0 mt-1 text-sm text-secondary">
                Sign in to Modrinth to access the website features.
            </p>

            <div v-if="modrinthUser" class="mt-4 flex items-center gap-3">
                <p class="font-semibold">{{ modrinthUser.user_id }}</p>
                <button class="btn btn-red btn-small" @click="handleLogout">
                    Sign Out
                </button>
            </div>
            <div v-else class="mt-4">
                <button class="btn btn-brand" @click="handleLogin">
                    Sign In with Modrinth
                </button>
            </div>
        </div>

    </div>
</template>

<script setup lang="ts">
import {injectNotificationManager, StyledInput} from '@modrinth/ui'
import { ref, onMounted, onUnmounted } from 'vue'

import {
    users,
    remove_user,
    set_default_user,
    create_offline_user,
    minecraft_login,
} from '@/helpers/auth.js'
import { login, logout, get as getModrinthUser } from '@/helpers/mr_auth.ts'
import { generatePlayerHeadBlob } from '@/helpers/rendering/batch-skin-renderer.ts'

const notificationManager = injectNotificationManager()


// ===== 辅助函数 =====

/** 获取玩家名的首字母 */
function getInitial(name: string): string {
    return name.charAt(0).toUpperCase()
}

/** 为离线账户生成一致的头像背景色 */
function getOfflineAvatarColor(name: string): string {
    let hash = 0
    for (let i = 0; i < name.length; i++) {
        hash = name.charCodeAt(i) + ((hash << 5) - hash)
    }
    const hue = Math.abs(hash % 360)
    return `hsl(${hue}, 60%, 45%)`
}

function notifyError(msg: string) {
    notificationManager.addNotification({ title: 'Error', text: msg, type: 'error' })
}

function notifySuccess(msg: string) {
    notificationManager.addNotification({ title: 'Success', text: msg, type: 'success' })
}

// ===== 状态 =====
const minecraftUsers = ref<any[]>([])
const modrinthUser = ref<any>(null)
const loading = ref(true)
const newOfflineUsername = ref('')

let creatingOffline = ref(false)

// 每个正版账户的渲染头像 URL 缓存（key = UUID）
const headUrlCache = new Map<string, string>()

/** 为指定账户生成并缓存渲染头像 */
async function ensureHeadUrl(user: any): Promise<string> {
    const uuid = user.profile.id
    if (!uuid) return ''

    // 已有缓存则直接返回
    const cached = headUrlCache.get(uuid)
    if (cached) return cached

    // 离线账户不需要渲染头像
    if (user.access_token === 'OFFLINE') return ''

    // 从 profile.skins 中获取当前装备的皮肤纹理 URL
    const skinUrl = user.profile?.skins?.[0]?.url
    if (!skinUrl) return ''

    try {
        // generatePlayerHeadBlob 直接接受纹理 URL，内部用 Image + Canvas 提取头部像素
        const headBlob = await generatePlayerHeadBlob(skinUrl, 40)
        const headUrl = URL.createObjectURL(headBlob)
        headUrlCache.set(uuid, headUrl)
        return headUrl
    } catch (err) {
        console.warn(`Failed to generate head for ${uuid}:`, err)
        return ''
    }
}

/** 获取用户的渲染头像 URL */
function getUserHeadUrl(user: any): string {
    return headUrlCache.get(user.profile.id) ?? ''
}

// ===== Minecraft 账户 =====
async function loadUsers() {
    type MinecraftUser = {
        profile: { id: string; name: string }
        access_token: string
        active: boolean
    }
    try {
        const rawUsers = await users()
        minecraftUsers.value = ([...rawUsers] as any).sort((a: { active: any; access_token: string; profile: { name: any } }, b: { active: any; access_token: string; profile: { name: any } }) => {
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

        // 为每个正版账户预加载渲染头像
        const onlineUsers = minecraftUsers.value.filter(
            (u: any) => u.access_token !== 'OFFLINE'
        )
        await Promise.all(onlineUsers.map(u => ensureHeadUrl(u)))
    } catch (err) {
        notifyError(err instanceof Error ? err.message : String(err))
    }
}

async function onCreateOffline() {
    const username = newOfflineUsername.value.trim()
    creatingOffline = ref(true)
    if (!username) return

    try {
        await create_offline_user(username)
        newOfflineUsername.value = ''
        await loadUsers()
        notifySuccess('Offline account created successfully')
    } catch (err) {
        notifyError(err instanceof Error ? err.message : String(err))
    }
    creatingOffline = ref(false)
}

async function onRemoveUser(uuid: string) {
    try {
        // 清理该用户的缓存头像
        const oldUrl = headUrlCache.get(uuid)
        if (oldUrl) {
            URL.revokeObjectURL(oldUrl)
            headUrlCache.delete(uuid)
        }
        await remove_user(uuid)
        await loadUsers()
    } catch (err) {
        notifyError(err instanceof Error ? err.message : String(err))
    }
}

async function onSetDefault(uuid: string) {
    try {
        await set_default_user(uuid)
        await loadUsers()
    } catch (err) {
        notifyError(err instanceof Error ? err.message : String(err))
    }
}



// ===== Modrinth 账户 =====
async function loadModrinthUser() {
    try {
        const data = await getModrinthUser()
        modrinthUser.value = data
    } catch (err) {
        modrinthUser.value = null
    }
}

async function handleLogin() {
    try {
        await login()
        await loadModrinthUser()
    } catch (err) {
        notifyError(err instanceof Error ? err.message : String(err))
    }
}

async function handleLogout() {
    try {
        await logout()
        modrinthUser.value = null
        notifySuccess('Signed out of Modrinth')
    } catch (err) {
        notifyError(err instanceof Error ? err.message : String(err))
    }
}

async function handleMinecraftLogin() {
    try {
        await minecraft_login()
        await loadUsers()
        notifySuccess('Minecraft 正版登录成功')
    } catch (err) {
        notifyError(err instanceof Error ? err.message : String(err))
    }
}

// ===== 初始化 =====
onMounted(async () => {
    await Promise.all([loadUsers(), loadModrinthUser()])
    loading.value = false
})

// ===== 清理 =====
onUnmounted(() => {
    // 释放所有缓存的 Blob URL
    for (const url of headUrlCache.values()) {
        URL.revokeObjectURL(url)
    }
    headUrlCache.clear()
})
</script>
