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
	type: PluginType
	api_version: number
	entry?: string | null
	permissions: string[]
	settings: PluginSettingField[]
	sidecar?: unknown
}

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
}
