// Persistence for the library's per-sort-mode ascending/descending state.
//
// The state lives in `<appdata>/custom_backgrounds/celestial_settings.json`
// under the `library_sort_directions` key, alongside the other Celestial-only
// settings (background blur, hue, import banner).
import { invoke } from '@tauri-apps/api/core'

export type LibrarySortDirection = 'asc' | 'desc'

/** Raw shape stored on disk: sort mode name -> 'asc' | 'desc'. */
export type LibrarySortDirections = Record<string, LibrarySortDirection>

function normalize(raw: Record<string, string>): LibrarySortDirections {
	const directions: LibrarySortDirections = {}
	for (const [sortMode, direction] of Object.entries(raw ?? {})) {
		if (direction === 'asc' || direction === 'desc') {
			directions[sortMode] = direction
		}
	}
	return directions
}

/**
 * Reads the saved directions. Returns an empty object when nothing has been
 * saved yet, so callers fall back to each sort mode's default direction.
 */
export async function load_library_sort_directions(): Promise<LibrarySortDirections> {
	try {
		return normalize(await invoke<Record<string, string>>('load_library_sort_directions'))
	} catch (error) {
		console.error('Failed to load library sort directions:', error)
		return {}
	}
}

/** Writes the full direction map back, leaving other settings in the file untouched. */
export async function save_library_sort_directions(
	directions: LibrarySortDirections,
): Promise<void> {
	try {
		await invoke('save_library_sort_directions', { directions })
	} catch (error) {
		console.error('Failed to save library sort directions:', error)
	}
}
