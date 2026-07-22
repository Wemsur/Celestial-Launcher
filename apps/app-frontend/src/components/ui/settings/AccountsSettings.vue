<template>
    <div class="flex flex-col gap-6">

        <!-- 第一部分：创建离线账户 -->
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">离线登录</h2>
            <p class="m-0 mt-1 text-sm text-secondary">
                玩家名应符合 3-16 字符 (仅包含 字母、数字、下划线)
            </p>

            <div class="mt-4 flex gap-2">
                <StyledInput
                    :model-value="newOfflineUsername"
                    @update:model-value="newOfflineUsername = $event"
                    placeholder="请输入玩家名..."
                    wrapper-class="flex-1 w-full"
                    :disabled="creatingOffline"
                    clearable
                    @keyup.enter="onCreateOffline"
                />
                <button
                    class="btn btn-brand"
                    @click="onCreateOffline"
                    :disabled="!newOfflineUsername.trim()"
                >
                    添加
                </button>
            </div>
        </div>

        <div class="mb-0">
            <h2 class="m-0 text-lg font-semibold text-contrast">微软登录</h2>
            <button
                    class="btn btn-brand"
                    @click="handleMinecraftLogin"
                >
                添加正版账户
            </button>
        </div>

        <hr class="bg-button-border border-none h-[1px]" />

        <!-- 第二部分：Minecraft 账户列表 -->
        <div style="margin-top: -30px; margin-bottom: -30px;">
            <h2 class="m-0 text-lg font-semibold text-contrast">Minecraft 账户</h2>

            <div v-if="loading" class="mt-4 text-secondary">加载中...</div>

            <div v-else-if="minecraftUsers.length === 0" class="mt-4">
                <EmptyState type="empty" heading="无账号" description="请先添加正版或离线账号" />
            </div>

            <div v-else class="mt-4 flex flex-col gap-3">
                <div
                    v-for="user in minecraftUsers"
                    :key="user.profile.id"
                    style="border: 2px solid color-mix(in srgb, var(--color-text-primary) 10%, transparent)"
                    class="flex items-center justify-between rounded-lg p-3 bg-surface-2 border-surface-5 border-spacing-1"
                    :class="{ 'ring-2 ring-brand': user.active }"
                >
                    <div class="flex items-center grid-when-huge w-96 gap-2 flex-1 overflow-y-auto">
                        <!-- 头像：离线账户用 CSS 首字母，正版账户用皮肤渲染头像 -->
                        <div
                            v-if="user.access_token === 'OFFLINE'"
                            class="w-14 h-14 rounded flex items-center justify-center text-white font-bold text-primary mr-3 shrink-0"
                            style="font-size: 1.6pc"
                            :style="{ backgroundColor: getOfflineAvatarColor(user.profile.name) }"
                        >
                            {{ getInitial(user.profile.name) }}
                        </div>
                        <img
                            v-else
                            :src="getUserHeadUrl(user)"
                            class="w-14 h-14 mr-3 rounded shrink-0"
                            alt=""
                        />
                        <div>
                            <div class="text-lg text-contrast font-bold truncate smart-clickable:underline-on-hover">
                                {{ user.profile.name }}
                            </div>
                            <div class="flex items-center gap-2 text-sm text-secondary">
                                {{ user.profile.id }}
                            </div>
                            <div class="flex items-center gap-2 text-sm" :class="user.access_token === 'OFFLINE' ? 'text-primary' : 'text-orange'">
                                {{ user.access_token === 'OFFLINE' ? '离线账号' : '正版账号' }}
                            </div>
                        </div>
                    </div>
                    <div class="flex gap-2">
                        <button
                            v-if="!user.active"
                            class="btn btn-standard btn-small"
                            @click="onSetDefault(user.profile.id)"
                        >
                            使用
                        </button>
                        <button
                            class="btn btn-standard btn-small btn-red"
                            @click="onRemoveUser(user.profile.id)"
                        >
                            移除
                        </button>
                    </div>
                </div>
            </div>
        </div>

        <hr class="bg-button-border border-none h-[1px]" />

        <!-- 第三部分：Modrinth 账户 -->
        <div>
            <h2 class="m-0 text-lg font-semibold text-contrast">Modrinth 账户</h2>
            <p class="m-0 mt-1 text-sm text-secondary">
                登录Modrinth账户以开启好友功能
            </p>

            <div v-if="modrinthUser" class="mt-4 flex items-center gap-3">
                <Avatar :src="modrinthCredentials?.user?.avatar_url" alt="" size="32px" circle />
                <p class="font-semibold">
                    {{ modrinthCredentials?.user?.username }}</p>
                <button class="btn btn-red btn-small" @click="handleLogout">
                    登出
                </button>
            </div>
            <div v-else class="mt-4">
                <button class="btn btn-brand" @click="handleLogin">
                    登录Modrinth
                </button>
            </div>
        </div>

    </div>
</template>

<script setup lang="ts">
import {Avatar, injectNotificationManager, StyledInput} from '@modrinth/ui'
import { inject, ref, onMounted, onUnmounted } from 'vue'

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
const modrinthCredentials = inject('modrinthCredentials', null)



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
    notificationManager.addNotification({ title: '错误', text: msg, type: 'error' })
}

function notifySuccess(msg: string) {
    notificationManager.addNotification({ title: '成功', text: msg, type: 'success' })
}

// ===== 状态 =====
const minecraftUsers = ref<any[]>([])
const modrinthUser = ref<any>(null)
const loading = ref(true)
const newOfflineUsername = ref('')

const creatingOffline = ref(false)

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
    creatingOffline.value = true
    if (!username) return

    try {
        await create_offline_user(username)
        newOfflineUsername.value = ''
        await loadUsers()
        notifySuccess('离线账号创建成功')
    } catch (err) {
        notifyError(err instanceof Error ? err.message : String(err))
    }
    creatingOffline.value = false
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
        notifySuccess('已退出 Modrinth 登录')
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
