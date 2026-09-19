/** Mirrors `theseus::plugins` on the Rust side. */

export type PluginType = 'ui' | 'sidecar'

export type PermissionRisk = 'low' | 'high'

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
	manifest: PluginManifest | null
	error: string | null
}
