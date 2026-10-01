/**
 * The neutral content model shared by every content source.
 *
 * Modrinth is not privileged here: it is one source among many, and its data is
 * normalised into these shapes like any other source's. The launcher owns the
 * content-type set and how each type is installed; a source only declares which
 * types it serves, what it can do, and how to fetch a file.
 *
 * Manifest and summary fields are snake_case because they mirror the Rust
 * structs and the `manifest.json` a plugin author writes — the same convention
 * launcher UI plugins use. The provider methods are camelCase because they are
 * a JavaScript interface the source implements, not serialised data.
 */

/** The launcher's closed set of content types. A source may only serve these. */
export type ContentType =
	| 'mod'
	| 'modpack'
	| 'resourcepack'
	| 'shader'
	| 'datapack'
	| 'world'
	| 'skin'

/** Sort keys the launcher can ask for. */
export type ContentSort = 'relevance' | 'downloads' | 'follows' | 'newest' | 'updated'

/**
 * What a source can actually do. The launcher reads this to decide which UI to
 * show and which operations to enable; anything `false` degrades rather than
 * breaks (a source that cannot `resolve_download` still browses, its install
 * button just opens the project page instead).
 */
export interface SourceCapabilities {
	/** Free-text search. */
	search: boolean
	/** Listing content without a query (categories, newest, etc.). */
	browse: boolean
	/** A full project page with a description and gallery. */
	detail: boolean
	/** A version list for a project. */
	versions: boolean
	/** Produce a fetchable file for a project/version. */
	resolve_download: boolean
	/** Given installed items, report whether newer versions exist. */
	update_check: boolean
}

/** One field of the manual-entry form, for sources with no browsing of their own. */
export interface ManualEntryField {
	key: string
	label: string
	description?: string | null
	placeholder?: string | null
	/** A regex the value must match; shown as a hint when it does not. */
	pattern?: string | null
	/** Present when the field is a choice; the form renders a dropdown. */
	options: SourceSettingOption[]
	required?: boolean
}

/** Set when a source needs the user to sign in before it works. */
export interface SourceAuth {
	required: boolean
	login_url?: string | null
	help_url?: string | null
}

export type SourceSettingType = 'toggle' | 'text' | 'number' | 'select'

export interface SourceSettingOption {
	value: string
	label?: string | null
}

/** A setting the launcher renders as a form and stores for the source. */
export interface SourceSettingField {
	key: string
	label: string
	description?: string | null
	type: SourceSettingType
	default?: string | null
	options: SourceSettingOption[]
}

/** The manifest a source ships, as read and validated by the backend. */
export interface ContentSourceManifest {
	id: string
	name: string
	version: string
	description?: string | null
	author?: string | null
	homepage?: string | null
	api_version: number
	entry: string
	content_types: ContentType[]
	capabilities: SourceCapabilities
	/** Hosts this source may reach, as `example.com` or `*.example.com`. */
	hosts: string[]
	auth?: SourceAuth | null
	manual_entry: ManualEntryField[]
	settings: SourceSettingField[]
}

/** A source as the source manager sees it: the manifest, plus whether it is on. */
export interface SourceSummary {
	/** The manifest id, or the folder name when the manifest is unusable. */
	id: string
	dir_name: string
	path: string
	enabled: boolean
	manifest: ContentSourceManifest | null
	error: string | null
}

/** A search or browse request, in neutral terms. */
export interface ContentQuery {
	query?: string
	contentType: ContentType
	gameVersion?: string
	loader?: string
	sort: ContentSort
	/** 1-based. */
	page: number
	limit: number
	/** A source-specific category id, from `filters()` if the source offers one. */
	category?: string
	/** Opaque source-specific filter state, round-tripped from a previous call. */
	filters?: Record<string, unknown>
}

/** One card in a merged list. */
export interface ContentCard {
	sourceId: string
	/** The source's own id for this project; unique within the source. */
	id: string
	contentType: ContentType
	name: string
	summary?: string
	iconUrl?: string
	author?: string
	/** Raw download count; the launcher applies the source's multiplier when sorting. */
	downloads?: number
	follows?: number
	publishedAt?: string
	updatedAt?: string
	gameVersions?: string[]
	loaders?: string[]
	/** Badges the source wants shown, e.g. the site's own category name. */
	tags?: string[]
}

export interface ContentPage {
	items: ContentCard[]
	total: number
}

export interface ContentVersion {
	id: string
	name?: string
	versionNumber?: string
	gameVersions?: string[]
	loaders?: string[]
	publishedAt?: string
	downloads?: number
	channel?: string
}

/** A project page. Every field past the card is optional; the page degrades. */
export interface ContentProject extends ContentCard {
	body?: string
	gallery?: { url: string; title?: string }[]
	license?: string
	links?: { label: string; url: string }[]
	versions?: ContentVersion[]
}

/** Everything the launcher needs to fetch one file. */
export interface DownloadDescriptor {
	url: string
	/** Headers the fetch needs (Referer, User-Agent, Cookie), set by the source. */
	headers?: Record<string, string>
	/** Suggested file name; the launcher falls back to the URL's last segment. */
	fileName?: string
	/** Hex digest when the source can supply one; omitted means record-only. */
	hash?: { algorithm: 'sha1' | 'sha512'; value: string }
}

/** One installed item handed to a source for an update check. */
export interface UpdateQueryItem {
	id: string
	contentType: ContentType
	/** The version identifier this item is currently on, as the source gave it. */
	currentVersionId?: string
	gameVersion?: string
	loader?: string
	/**
	 * The rule the item was installed under, so the source can re-match the same
	 * asset (e.g. which release asset a GitHub project should track).
	 */
	installRule?: Record<string, unknown>
}

export interface UpdateResult {
	id: string
	/** The newer version, or undefined when the item is current. */
	latestVersionId?: string
	latestVersionName?: string
}

/**
 * The functions a source provides. Implement only the ones your
 * `capabilities` claim; the launcher calls nothing else.
 */
export interface ContentSourceProvider {
	search?(request: ContentQuery): Promise<ContentPage>
	browse?(request: ContentQuery): Promise<ContentPage>
	detail?(id: string, contentType: ContentType): Promise<ContentProject>
	versions?(id: string, request: ContentQuery): Promise<ContentVersion[]>
	resolveDownload?(id: string, versionId?: string): Promise<DownloadDescriptor>
	checkUpdate?(items: UpdateQueryItem[]): Promise<UpdateResult[]>
}
