import type { Ref } from 'vue'

import type LibrarySelectModal from '@/components/ui/modal/LibrarySelectModal.vue'
import { library_list } from '@/helpers/library'

export type LibrarySelection = {
	path: string
	format: 'modrinth' | 'minecraft'
}

type ModalRef = Ref<InstanceType<typeof LibrarySelectModal> | undefined>

let modalRef: ModalRef | null = null

/** Registered once from App.vue after the shared modal is mounted. */
export function setLibraryPickerModal(ref: ModalRef) {
	modalRef = ref
}

/**
 * Asks the user which library an install should target.
 *
 * Resolves to `null` in two very different situations, which callers must
 * distinguish by intent:
 * - the user cancelled → abort the install (see {@link pickInstallLibrary}'s
 *   `cancelled` flag below), or
 * - there is nothing to choose from (0 or 1 library) → fall through to the
 *   backend default.
 *
 * To keep call sites simple we return `{ cancelled, library }` instead.
 */
export async function pickInstallLibrary(): Promise<{
	cancelled: boolean
	library: LibrarySelection | null
}> {
	let libraryCount = 0
	try {
		const config = await library_list()
		libraryCount = config.libraries.length
	} catch {
		return { cancelled: false, library: null }
	}

	// Nothing meaningful to pick: single library installs there anyway, and zero
	// libraries falls back to the default library on the backend.
	if (libraryCount <= 1 || !modalRef?.value) {
		return { cancelled: false, library: null }
	}

	const library = await modalRef.value.pick()
	return { cancelled: library === null, library }
}
