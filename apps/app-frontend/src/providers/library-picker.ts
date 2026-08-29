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
 * Always resolves to a concrete library when one can be determined, so the
 * install takes the same JSON-backed path no matter how many libraries exist.
 * Returning `null` here used to mean "let the backend pick a default", but the
 * backend's default is the DB path (`config_dir/profiles/<name>`), which lands
 * the instance *outside* every registered library — `find_json_instance` then
 * cannot resolve it at completion time and `install_stage` is never advanced
 * past `*_installing`, leaving the card spinning forever.
 *
 * `library` is only `null` when the user cancelled (`cancelled: true`) or when
 * `libraries.json` could not be read at all.
 */
export async function pickInstallLibrary(): Promise<{
	cancelled: boolean
	library: LibrarySelection | null
}> {
	let config: Awaited<ReturnType<typeof library_list>>
	try {
		config = await library_list()
	} catch {
		return { cancelled: false, library: null }
	}

	const libraries = config.libraries ?? []
	const toSelection = (library: (typeof libraries)[number]): LibrarySelection => ({
		path: library.path,
		format: library.type,
	})

	// Only one library (or no modal mounted yet): nothing to ask, but still
	// resolve it explicitly instead of falling through to the backend default.
	if (libraries.length <= 1 || !modalRef?.value) {
		const active =
			libraries.find((library) => library.path === config.active_library_path) ?? libraries[0]
		return { cancelled: false, library: active ? toSelection(active) : null }
	}

	const library = await modalRef.value.pick()
	return { cancelled: library === null, library }
}
