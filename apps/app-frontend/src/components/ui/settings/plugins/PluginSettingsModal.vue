<script setup lang="ts">
import {
	Button,
	DropdownSelect,
	Input,
	NewModal,
	Toggle,
	useVIntl,
	defineMessages,
	injectNotificationManager,
} from '@modrinth/ui'
import { computed, ref, watch } from 'vue'

import { pluginSettingsGet, pluginSettingsSet } from '@/plugin-host/ipc'
import { loadPlugins, unloadPlugin } from '@/plugin-host'
import type { PluginSettingField, PluginSummary } from '@/plugin-host/types'

const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()

const props = defineProps<{ plugin: PluginSummary | null }>()

const modal = ref<InstanceType<typeof NewModal> | null>(null)

// Current values, keyed by setting key. Seeded from the plugin's saved values,
// falling back to each field's declared default.
const values = ref<Record<string, string>>({})

const messages = defineMessages({
	title: {
		id: 'app.settings.plugins.settings-modal.title',
		defaultMessage: 'Plugin settings',
	},
	noSettings: {
		id: 'app.settings.plugins.settings-modal.none',
		defaultMessage: 'This plugin has no settings.',
	},
	saved: {
		id: 'app.settings.plugins.settings-modal.saved',
		defaultMessage: 'Changes are saved as you make them.',
	},
	done: {
		id: 'app.settings.plugins.settings-modal.done',
		defaultMessage: 'Done',
	},
})

const fields = computed<PluginSettingField[]>(() => props.plugin?.manifest?.settings ?? [])

/** The slot a plugin's own settings UI can render into. Named after the id so a
 *  plugin can target `plugin-settings:<its id>`. */
const customSlotId = computed(() =>
	props.plugin ? `plugin-settings:${props.plugin.id}` : undefined,
)

async function loadValues() {
	if (!props.plugin) return
	try {
		const saved = await pluginSettingsGet(props.plugin.id)
		const next: Record<string, string> = {}
		for (const field of fields.value) {
			next[field.key] = saved[field.key] ?? field.default ?? defaultFor(field)
		}
		values.value = next
	} catch (error) {
		handleError(error)
	}
}

function defaultFor(field: PluginSettingField): string {
	if (field.type === 'toggle') return 'false'
	if (field.type === 'number') return '0'
	if (field.type === 'select') return field.options[0]?.value ?? ''
	return ''
}

async function update(field: PluginSettingField, value: string) {
	if (!props.plugin) return
	values.value = { ...values.value, [field.key]: value }
	try {
		await pluginSettingsSet(props.plugin.id, field.key, value)
		// A toggle usually changes what the plugin registers (which slots/pages
		// it shows), and that is decided at activation — so reload it. Text
		// settings are read on use and need no reload (and reloading on every
		// keystroke would be a storm).
		if (field.type === 'toggle') {
			unloadPlugin(props.plugin.id)
			await loadPlugins()
		}
	} catch (error) {
		handleError(error)
	}
}

function show() {
	void loadValues()
	modal.value?.show()
}

function hide() {
	modal.value?.hide()
}

// Reload whenever the target plugin changes while the modal stays mounted.
watch(() => props.plugin?.id, loadValues)

defineExpose({ show, hide })
</script>

<template>
	<NewModal ref="modal" :header="formatMessage(messages.title)">
		<div class="flex flex-col gap-4 w-[480px] max-w-full">
			<div v-if="plugin" class="flex flex-col gap-1">
				<h2 class="m-0 text-lg font-semibold text-contrast">
					{{ plugin.manifest?.name ?? plugin.dir_name }}
				</h2>
				<span v-if="plugin.manifest?.version" class="text-sm text-secondary">
					v{{ plugin.manifest.version }}
				</span>
			</div>

			<p v-if="fields.length === 0" class="m-0 text-secondary">
				{{ formatMessage(messages.noSettings) }}
			</p>

			<div v-else class="flex flex-col gap-4">
				<div v-for="field in fields" :key="field.key" class="flex flex-col gap-1">
					<div class="flex items-center justify-between gap-4">
						<label class="text-sm font-medium text-contrast">
							{{ field.label }}
						</label>

						<Toggle
							v-if="field.type === 'toggle'"
							:model-value="values[field.key] === 'true'"
							@update:model-value="(value: boolean) => update(field, String(value))"
						/>
						<DropdownSelect
							v-else-if="field.type === 'select'"
							:model-value="values[field.key]"
							:options="field.options.map((option) => option.value)"
							:display-name="
								(value: string) =>
									field.options.find((option) => option.value === value)?.label ?? value
							"
							name="Plugin setting"
							@update:model-value="(value: string) => update(field, value)"
						/>
					</div>

					<Input
						v-if="field.type === 'text' || field.type === 'number'"
						:model-value="values[field.key]"
						:type="field.type === 'number' ? 'number' : 'text'"
						wrapper-class="w-full"
						@update:model-value="(value: string) => update(field, value)"
					/>

					<span v-if="field.description" class="text-xs text-secondary">
						{{ field.description }}
					</span>
				</div>
			</div>

			<!-- A plugin may render its own controls here for anything the
			     declarative fields cannot express. -->
			<div v-if="customSlotId" :data-plugin-slot="customSlotId"></div>

			<div class="flex items-center justify-between gap-2 pt-2">
				<span class="text-xs text-secondary">{{ formatMessage(messages.saved) }}</span>
				<Button @click="hide">{{ formatMessage(messages.done) }}</Button>
			</div>
		</div>
	</NewModal>
</template>
