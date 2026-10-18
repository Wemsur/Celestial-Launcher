import { reactive, type ShallowRef } from 'vue'
import type { ComponentExposed } from 'vue-component-type-helpers'

import { defineMessages, useVIntl } from '#ui/composables/i18n'

import { createContext } from '../../../providers'
import {
	injectLibraryManagement,
	type LibraryFormat,
} from '../../../providers/library-management'
import type { MultiStageModal } from '../../base'

export const addLibraryMessages = defineMessages({
	chooseFormatTitle: {
		id: 'library-modal.format.title',
		defaultMessage: 'What kind of library is this?',
	},
	detailsTitle: {
		id: 'library-modal.details.title',
		defaultMessage: 'Add a library',
	},
	modrinthFormatTitle: {
		id: 'library-modal.format.modrinth.title',
		defaultMessage: 'Modrinth format',
	},
	modrinthFormatDescription: {
		id: 'library-modal.format.modrinth.description',
		defaultMessage:
			'Each instance is its own folder. Works with the Modrinth app, the Axolotl launcher, Celestial Launcher and others.',
	},
	minecraftFormatTitle: {
		id: 'library-modal.format.minecraft.title',
		defaultMessage: '.minecraft format',
	},
	minecraftFormatDescription: {
		id: 'library-modal.format.minecraft.description',
		defaultMessage:
			'Versions live in a versions folder, as in a .minecraft directory. Works with PCL, HMCL, Baka XL and most other launchers.',
	},
	nameLabel: {
		id: 'library-modal.details.name.label',
		defaultMessage: 'Library name (optional)',
	},
	namePlaceholder: {
		id: 'library-modal.details.name.placeholder',
		defaultMessage: 'Uses the folder name when left empty',
	},
	pathLabel: {
		id: 'library-modal.details.path.label',
		defaultMessage: 'Library folder',
	},
	pathPlaceholder: {
		id: 'library-modal.details.path.placeholder',
		defaultMessage: 'Choose a folder or type a path',
	},
	browse: {
		id: 'library-modal.details.browse',
		defaultMessage: 'Browse for a folder',
	},
	formatLabel: {
		id: 'library-modal.details.format',
		defaultMessage: 'Format',
	},
	addButton: {
		id: 'library-modal.action.add',
		defaultMessage: 'Add / Create',
	},
	back: {
		id: 'library-modal.action.back',
		defaultMessage: 'Back',
	},
})

export interface AddLibraryContextValue {
	formatMessage: ReturnType<typeof useVIntl>['formatMessage']
	messages: typeof addLibraryMessages
	format: LibraryFormat | null
	name: string
	path: string
	submitting: boolean
	/** Moves to the next stage; the format choice is the only thing that needs it. */
	advance: () => void
	browse: () => Promise<void>
	addLibrary: () => Promise<string>
	reset: () => void
}

export const [injectAddLibraryContext, provideAddLibraryContext] =
	createContext<AddLibraryContextValue>('AddLibraryModal')

/**
 * The add-library dialog's state, shared by its two stages.
 *
 * A plain reactive object rather than a bag of refs: `MultiStageModal` renders
 * each stage with no props at all, so both stages reach the same four fields
 * through injection — the same way the creation flow's stages do.
 */
export function createAddLibraryContext(
	modal: ShallowRef<ComponentExposed<typeof MultiStageModal> | null>,
): AddLibraryContextValue {
	const { formatMessage } = useVIntl()
	const libraryManagement = injectLibraryManagement()

	// `modal` is read through a function rather than handed over as a ref:
	// `reactive()` unwraps refs, so a `modal` property would arrive as the
	// component instance and `modal.value?.nextStage()` would silently do
	// nothing — the format stage would never move on.
	const advance = () => modal.value?.nextStage()

	return reactive({
		formatMessage,
		messages: addLibraryMessages,
		format: null as LibraryFormat | null,
		name: '',
		path: '',
		submitting: false,
		advance,
		async browse() {
			const chosen = await libraryManagement.pickFolder()
			if (chosen) this.path = chosen
		},
		async addLibrary(): Promise<string> {
			if (!this.format) {
				throw new Error('A library format has to be chosen first.')
			}
			const trimmedPath = this.path.trim()
			await libraryManagement.addLibrary({
				path: trimmedPath,
				format: this.format,
				name: this.name.trim(),
			})
			return trimmedPath
		},
		reset() {
			this.format = null
			this.name = ''
			this.path = ''
			this.submitting = false
		},
	}) as AddLibraryContextValue
}
