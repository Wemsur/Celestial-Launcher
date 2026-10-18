import { createContext } from '.'

/** The two layouts a library folder can be in. */
export type LibraryFormat = 'modrinth' | 'minecraft'

export interface LibraryEntry {
	path: string
	/** The name the user gave the library, empty when it was never named. */
	name: string
	format: LibraryFormat
}

export interface AddLibraryInput {
	path: string
	format: LibraryFormat
	/** Optional display name; an empty string means "use the folder name". */
	name: string
}

/**
 * Registering a library folder, from wherever the user asked for it.
 *
 * The dialog that does this is mounted once and reached from every entry point,
 * so the app only has to supply these three operations.
 */
export interface LibraryManagementProvider {
	/** Registers a library folder and reloads the list. */
	addLibrary: (input: AddLibraryInput) => Promise<void>
	/** Opens the platform's own directory picker; `null` when cancelled. */
	pickFolder: () => Promise<string | null>
	/** Every library currently registered, read fresh. */
	libraries: () => Promise<LibraryEntry[]>
}

export const [injectLibraryManagement, provideLibraryManagement] =
	createContext<LibraryManagementProvider>('LibraryManagement')
