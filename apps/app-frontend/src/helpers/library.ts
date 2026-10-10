import { invoke } from '@tauri-apps/api/core'

export type InstanceFormat = 'modrinth' | 'minecraft'

export type LibraryInfo = {
	name: string
	path: string
	type: InstanceFormat
}

export type LibrariesConfig = {
	libraries: LibraryInfo[]
	migrated: boolean
	active_library_path?: string | null
}

export async function library_list(): Promise<LibrariesConfig> {
	return await invoke('plugin:instance|library_list')
}

export async function library_add(
	path: string,
	format: InstanceFormat,
	name?: string,
): Promise<void> {
	return await invoke('plugin:instance|library_add', { path, format, name })
}

export async function library_remove(path: string): Promise<void> {
	return await invoke('plugin:instance|library_remove', { path })
}

export async function library_set_active(path: string): Promise<void> {
	return await invoke('plugin:instance|library_set_active', { path })
}

export async function library_update_name(path: string, name: string): Promise<void> {
	return await invoke('plugin:instance|library_update_name', { path, name })
}

/** Persists the library order in `libraries.json` (drag-and-drop on the home nav). */
export async function library_reorder(paths: string[]): Promise<void> {
	return await invoke('plugin:instance|library_reorder', { paths })
}

export async function library_default_path(): Promise<string> {
	return await invoke('plugin:instance|library_default_path')
}

/**
 * What a library is called wherever one is listed — the home nav's tabs and the
 * install picker alike: the name the user gave it, else "默认库" for the
 * launcher's own path, else the folder it lives in. `defaultPath` is optional
 * so a caller that could not read it still gets a usable label.
 */
export function libraryDisplayLabel(
	library: { name?: string | null; path: string },
	defaultPath?: string | null,
): string {
	if (library.name) return library.name

	const normalize = (value: string) =>
		value.replace(/\\/g, '/').toLowerCase().replace(/\/+$/, '')
	if (defaultPath) {
		const normalizedDefault = normalize(defaultPath)
		const normalizedPath = normalize(library.path)
		// The default library holds the instance folders, so its own path and the
		// `profiles` folder inside it are the same library written two ways.
		if (normalizedPath === normalizedDefault || normalizedPath === `${normalizedDefault}/profiles`) {
			return '默认库'
		}
	}

	const segments = library.path.split(/[\\/]/).filter(Boolean)
	return segments[segments.length - 1] ?? library.path
}

