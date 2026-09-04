<template>
	<SharedLanguageSettings
		ref="languageSettings"
		product="app"
		selector="dropdown"
		:persist-locale="persistLocale"
	/>

	<section class="mt-8 border-0 border-t border-solid border-divider pt-6">
		<h2 class="m-0 text-xl font-semibold text-contrast">
			{{ formatMessage(messages.translationTitle) }}
		</h2>
		<p class="m-0 mt-1">{{ formatMessage(messages.translationDescription) }}</p>
		<div class="mt-4 flex flex-col gap-6">
			<div class="flex items-center justify-between gap-4">
				<div>
					<h3 class="m-0 text-lg font-semibold text-contrast">
						{{ formatMessage(messages.translationAutoTitle) }}
					</h3>
					<p class="m-0 mt-1">{{ formatMessage(messages.translationAutoDescription) }}</p>
				</div>
				<Toggle id="translation-auto-enable" v-model="autoEnableTranslation" />
			</div>

			<div class="flex items-center justify-between gap-4">
				<div>
					<h3 class="m-0 text-lg font-semibold text-contrast">
						{{ formatMessage(messages.translationServiceTitle) }}
					</h3>
					<p class="m-0 mt-1">{{ formatMessage(messages.translationServiceDescription) }}</p>
				</div>
				<DropdownSelect
					v-model="selectedService"
					name="translation-service"
					class="max-w-[16rem]"
					:options="serviceOptions"
					:display-name="serviceLabel"
				/>
			</div>

			<div class="flex items-center justify-between gap-4">
				<div>
					<h3 class="m-0 text-lg font-semibold text-contrast">
						{{ formatMessage(messages.translationCacheTitle) }}
					</h3>
					<p class="m-0 mt-1">{{ formatMessage(messages.translationCacheDescription) }}</p>
				</div>
				<Button :disabled="clearing" @click="clearTranslations">
					<TrashIcon />
					{{ formatMessage(messages.translationCacheButton) }}
				</Button>
			</div>
		</div>
	</section>
</template>

<script setup lang="ts">
import { TrashIcon } from '@modrinth/assets'
import {
	Button,
	defineMessages,
	DropdownSelect,
	LanguageSettings as SharedLanguageSettings,
	Toggle,
	useVIntl,
} from '@modrinth/ui'
import { computed, inject, onBeforeUnmount, onMounted, ref } from 'vue'

import { useContentTranslation } from '@/composables/use-content-translation.ts'
import { get, set } from '@/helpers/settings.ts'
import { isTranslationServiceId, TRANSLATION_PROVIDERS } from '@/helpers/translation/services'
import { appSettingsModalContextKey } from '@/providers/app-settings-modal'

const { formatMessage } = useVIntl()
const settingsModal = inject(appSettingsModalContextKey, null)
const languageSettings = ref<InstanceType<typeof SharedLanguageSettings> | null>(null)

onMounted(() => {
	settingsModal?.registerUnsavedChangesController({
		hasChanges: () => languageSettings.value?.hasChanges ?? false,
		getOriginal: () => languageSettings.value?.originalState ?? {},
		getModified: () => languageSettings.value?.modifiedState ?? {},
		isSaving: () => languageSettings.value?.saving ?? false,
		reset: () => languageSettings.value?.reset(),
		save: () => languageSettings.value?.save(),
	})
})

onBeforeUnmount(() => {
	settingsModal?.registerUnsavedChangesController(null)
})

async function persistLocale(locale: string): Promise<void> {
	const settings = await get()
	if (settings.locale === locale) return
	await set({ ...settings, locale })
}

// The translation settings save themselves the moment they change, so they are
// deliberately outside the language form's unsaved-changes controller above.
const { serviceId, autoEnable, setService, setAutoEnable, clearCache } = useContentTranslation()

const serviceOptions = TRANSLATION_PROVIDERS.map((provider) => provider.id)

function serviceLabel(id: string): string {
	return TRANSLATION_PROVIDERS.find((provider) => provider.id === id)?.label ?? id
}

const selectedService = computed<string>({
	get: () => serviceId.value,
	set: (id) => {
		if (isTranslationServiceId(id)) void setService(id)
	},
})

const autoEnableTranslation = computed<boolean>({
	get: () => autoEnable.value,
	set: (value) => void setAutoEnable(value),
})

const clearing = ref(false)

async function clearTranslations(): Promise<void> {
	clearing.value = true
	try {
		await clearCache()
	} finally {
		clearing.value = false
	}
}

const messages = defineMessages({
	translationTitle: {
		id: 'app.settings.translation.title',
		defaultMessage: 'Content translation',
	},
	translationDescription: {
		id: 'app.settings.translation.description',
		defaultMessage:
			'The translate button on the discover and project pages translates content written by authors into the launcher language. All services below are free and need no API key of your own.',
	},
	translationAutoTitle: {
		id: 'app.settings.translation.auto.title',
		defaultMessage: 'Translate automatically',
	},
	translationAutoDescription: {
		id: 'app.settings.translation.auto.description',
		defaultMessage:
			'Turn translation on by itself when you open the discover pages, instead of pressing the button every time.',
	},
	translationServiceTitle: {
		id: 'app.settings.translation.service.title',
		defaultMessage: 'Translation service',
	},
	translationServiceDescription: {
		id: 'app.settings.translation.service.description',
		defaultMessage:
			'If one service is unreachable or refuses requests, pick another. Applies immediately.',
	},
	translationCacheTitle: {
		id: 'app.settings.translation.cache.title',
		defaultMessage: 'Translation cache',
	},
	translationCacheDescription: {
		id: 'app.settings.translation.cache.description',
		defaultMessage: 'Translations are kept for 7 days so the same text is only fetched once.',
	},
	translationCacheButton: {
		id: 'app.settings.translation.cache.button',
		defaultMessage: 'Clear cache',
	},
})
</script>
