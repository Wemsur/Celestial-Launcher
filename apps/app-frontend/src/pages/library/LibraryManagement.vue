<script setup lang="ts">
import { injectNotificationManager } from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { onMounted,ref } from 'vue'

import { instance_listener } from '@/helpers/events.js'
import { list } from '@/helpers/instance'
import type { InstanceFormat,LibrariesConfig, LibraryInfo } from '@/helpers/library'

const { handleError } = injectNotificationManager()

const libraries = ref<LibraryInfo[]>([])
const migrated = ref(false)
const instances = ref<any[]>([])
const showAddModal = ref(false)
const addPath = ref('')
const addFormat = ref<InstanceFormat>('modrinth')
const isLoading = ref(true)

async function refreshLibraries() {
	try {
		const config = await invoke<LibrariesConfig>('plugin:instance|library_list')
		libraries.value = config.libraries
		migrated.value = config.migrated
	} catch (e) {
		handleError(e as Error)
	}
}

async function fetchInstances() {
	try {
		instances.value = await list()
	} catch (e) {
		handleError(e as Error)
	}
}

onMounted(async () => {
	await Promise.all([refreshLibraries(), fetchInstances()])
	isLoading.value = false
})

const unlistenInstance = await instance_listener(async () => {
	await Promise.all([refreshLibraries(), fetchInstances()])
})

async function addLibrary() {
	if (!addPath.value.trim()) return
	try {
		await invoke('plugin:instance|library_add', {
			path: addPath.value.trim(),
			format: addFormat.value,
		})
		await refreshLibraries()
		await fetchInstances()
		showAddModal.value = false
		addPath.value = ''
	} catch (e) {
		handleError(e as Error)
	}
}

async function pickFolder() {
	const result = await open({ directory: true, multiple: false })
	if (result && typeof result === 'string') {
		addPath.value = result
	}
}

async function removeLibrary(path: string) {
	try {
		await invoke('plugin:instance|library_remove', { path })
		await refreshLibraries()
		await fetchInstances()
	} catch (e) {
		handleError(e as Error)
	}
}
</script>

<template>
	<div class="p-6 flex flex-col gap-4">
		<div class="flex items-center justify-between">
			<h2 class="text-xl font-semibold">库管理</h2>
			<button
				class="px-3 py-1.5 bg-brand text-white rounded-md text-sm hover:bg-brand/90 transition-colors"
				@click="showAddModal = true"
			>
				+ 添加库
			</button>
		</div>

		<p class="text-sm text-gray-400">
			每个库对应一个文件夹，包含多个实例。导入后实例会自动扫描显示。
		</p>

		<!-- Add Modal -->
		<Teleport to="body">
			<transition name="fade">
				<div v-if="showAddModal" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
					<div class="bg-[#1e1e1e] border border-gray-700 rounded-lg p-6 w-96">
						<h3 class="text-lg font-medium mb-4">添加库</h3>
						<div class="flex gap-2 mb-3">
							<input
								v-model="addPath"
								type="text"
								placeholder="选择文件夹或输入路径"
								class="flex-1 px-3 py-2 bg-[#2a2a2a] border border-gray-600 rounded text-sm text-white"
							/>
							<button
								class="px-3 py-2 bg-[#3a3a3a] border border-gray-600 rounded text-sm hover:bg-[#4a4a4a]"
								@click="pickFolder"
							>
								浏览
							</button>
						</div>
						<div class="mb-4">
							<label class="text-sm text-gray-400 block mb-1">格式</label>
							<select
								v-model="addFormat"
								class="w-full px-3 py-2 bg-[#2a2a2a] border border-gray-600 rounded text-sm text-white"
							>
								<option value="modrinth">Modrinth (默认)</option>
								<option value="minecraft">Minecraft (.minecraft)</option>
							</select>
						</div>
						<div class="flex justify-end gap-2">
							<button
								class="px-3 py-1.5 text-sm text-gray-400 hover:text-white"
								@click="showAddModal = false"
							>
								取消
							</button>
							<button
								class="px-3 py-1.5 bg-brand text-white rounded text-sm hover:bg-brand/90"
								:disabled="!addPath.trim()"
								@click="addLibrary"
							>
								添加
							</button>
						</div>
					</div>
				</div>
			</transition>
		</Teleport>

		<!-- Library List -->
		<div class="space-y-2">
			<div v-if="libraries.length === 0" class="text-sm text-gray-500 py-4 text-center">
				暂无库，点击上方"添加库"导入文件夹
			</div>
			<div
				v-for="lib in libraries"
				:key="lib.path"
				class="flex items-center justify-between p-3 bg-[#2a2a2a] rounded border border-gray-700"
			>
				<div class="flex-1 min-w-0">
					<div class="font-medium truncate">{{ lib.path }}</div>
					<div class="text-xs text-gray-400">
						{{ lib.type === 'minecraft' ? 'Minecraft' : 'Modrinth' }} 格式
					</div>
				</div>
				<button
					class="ml-4 px-2 py-1 text-xs text-red-400 hover:text-red-300 border border-red-800/50 rounded"
					@click="removeLibrary(lib.path)"
				>
					删除
				</button>
			</div>
		</div>

		<!-- Instances from all libraries -->
		<div class="mt-4 pt-4 border-t border-gray-800">
			<h3 class="text-sm font-medium mb-2">已扫描实例 ({{ instances.length }})</h3>
			<div v-if="instances.length === 0" class="text-sm text-gray-500 py-2">
				未找到实例
			</div>
			<div v-for="inst in instances" :key="inst.id" class="text-sm py-1">
				{{ inst.name }} ({{ inst.path }})
			</div>
		</div>
	</div>
</template>

<style scoped>
.fade-enter-active, .fade-leave-active {
	transition: opacity 0.2s ease;
}
.fade-enter-from, .fade-leave-to {
	opacity: 0;
}
</style>
