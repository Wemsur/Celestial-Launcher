import { provideLibraryManagement } from '@modrinth/ui'
import { open } from '@tauri-apps/plugin-dialog'

import { library_add, library_list } from '@/helpers/library'

/**
 * The launcher's side of the add-library dialog: the dialog itself lives in
 * `@modrinth/ui`, and this is the only thing it needs from the app.
 *
 * Registering a library already emits `library_changed`, which the home page
 * and the creation modal both listen for, so nothing here has to tell them.
 */
export function setupLibraryManagementProvider() {
	provideLibraryManagement({
		async addLibrary({ path, format, name }) {
			await library_add(path, format, name || undefined)
		},
		async pickFolder() {
			const result = await open({ directory: true, multiple: false })
			return typeof result === 'string' ? result : null
		},
		async libraries() {
			const config = await library_list()
			return (config.libraries ?? []).map((library) => ({
				path: library.path,
				name: library.name,
				format: library.type,
			}))
		},
	})
}
