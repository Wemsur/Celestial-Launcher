<script setup lang="ts">
// A Vue single-file component. It is compiled by @vitejs/plugin-vue and its
// `vue` imports are redirected to the launcher's own Vue at build time, so it
// mounts inside the launcher like any first-party component.
import { ref } from 'vue'

const props = defineProps<{
	pluginName: string
	version: string
	opens: number
}>()

const emit = defineEmits<{ open: [] }>()

const clicks = ref(0)
</script>

<template>
	<div class="my-plugin-card" @click="emit('open')">
		<span class="my-plugin-title">{{ props.pluginName }}</span>
		<span class="my-plugin-detail">已启动 {{ props.opens }} 次 · v{{ props.version }}</span>
		<button class="my-plugin-button" @click.stop="clicks++">点击 {{ clicks }}</button>
	</div>
</template>
