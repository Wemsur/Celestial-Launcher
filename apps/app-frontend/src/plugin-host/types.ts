/** Mirrors `theseus::plugins` on the Rust side. */

export type PluginType = 'ui' | 'sidecar'

export type PermissionRisk = 'low' | 'high'

export type PluginSettingType = 'toggle' | 'text' | 'number' | 'select'

export interface PluginSettingOption {
	value: string
	label?: string | null
}

export interface PluginSettingField {
	key: string
	label: string
	description?: string | null
	type: PluginSettingType
	default?: string | null
	options: PluginSettingOption[]
}

export interface PluginPermission {
	kind: string
	scope: string | null
}

export interface PluginManifest {
	id: string
	name: string
	description?: string | null
	version: string
	author?: string | null
	homepage?: string | null
	/** A displayable image in the plugin's folder, e.g. `icon.png`. */
	icon?: string | null
	type: PluginType
	api_version: number
	entry?: string | null
	permissions: string[]
	settings: PluginSettingField[]
	sidecar?: unknown
}

/**
 * How a plugin's folder stands against the user's agreement to run it.
 *
 * `match` means the files are the ones that were agreed to — not that they are
 * trustworthy. Nothing here is a safety judgement; a plugin always runs with
 * the launcher's own privileges.
 */
export type ContentConsent = 'unknown' | 'match' | 'changed' | 'unavailable'

export interface PluginSummary {
	id: string
	dir_name: string
	path: string
	enabled: boolean
	granted: string[]
	pending: string[]
	high_risk_pending: string[]
	/** Every declared permission that needs the user's explicit approval. */
	high_risk: string[]
	manifest: PluginManifest | null
	error: string | null
	/** Route paths of this plugin's sidebar pages pinned to the nav rail. */
	pinned_pages: string[]
	/** The folder's digest, shown as its content fingerprint. */
	content_digest: string | null
	consent: ContentConsent
}
