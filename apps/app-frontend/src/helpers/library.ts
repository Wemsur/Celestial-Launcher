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
