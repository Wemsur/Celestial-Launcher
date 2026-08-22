import { createContext } from '@modrinth/ui'
import type { ComputedRef, Ref } from 'vue'

import type { GameInstance } from '@/helpers/types'

export interface InstanceSettingsUnsavedChangesController {
	hasChanges: () => boolean
	getOriginal: () => Record<string, unknown>
	getModified: () => Record<string, unknown>
	isSaving: () => boolean
	reset: () => void
	save: () => void | Promise<void>
}

export interface InstanceSettingsContext {
	instance: ComputedRef<GameInstance>
	offline?: boolean
	isMinecraftServer: Ref<boolean>
	onUnlinked: () => void
	closeModal?: () => void
	registerUnsavedChangesController: (
		controller: InstanceSettingsUnsavedChangesController | null,
	) => void
}

export const [injectInstanceSettings, provideInstanceSettings] =
	createContext<InstanceSettingsContext>('InstanceSettingsModal', 'instanceSettings')
