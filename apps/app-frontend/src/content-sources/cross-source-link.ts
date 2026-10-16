/**
 * Recognizing another catalogue's URL in a source's own links.
 *
 * A source that only *catalogues* content often has no file of its own — a
 * modpack entry on a wiki points at the pack's CurseForge page, say. Rather than
 * teach every source about every other source's install path, the launcher reads
 * the link: when it names a project on a source that is installed, the link
 * becomes an in-app jump to that source's own project page, where the real
 * install lives. A source therefore only has to publish the URL it already has.
 *
 * Only the two catalogues the launcher can actually navigate to are recognized.
 * A link to anything else (a Discord invite, a wiki, a file host) is left as the
 * external link it is.
 */

import { MODRINTH_SOURCE_ID } from './preferences'
import { installedManifest } from './catalog'
import type { ContentType } from './types'

export interface CrossSourceLink {
	/** The source the link points into. */
	sourceId: string
	/** That source's own id for the project — a slug, which is what a URL carries. */
	projectId: string
	/** The content type the URL's path segment implies. */
	contentType: ContentType
}

/**
 * CurseForge's URL path segments, which are also its content classes. The
 * segment is the singular site spelling; the launcher's type is what it maps to.
 */
const CURSEFORGE_PATHS: Record<string, ContentType> = {
	'mc-mods': 'mod',
	modpacks: 'modpack',
	'texture-packs': 'resourcepack',
	shaders: 'shader',
	'customization': 'datapack',
	'worlds': 'world',
}

/** Modrinth's URL path segments, mapped the same way. */
const MODRINTH_PATHS: Record<string, ContentType> = {
	mod: 'mod',
	modpack: 'modpack',
	resourcepack: 'resourcepack',
	shader: 'shader',
	datapack: 'datapack',
}

/**
 * The source a link points into, or undefined when it names no project the
 * launcher can open.
 *
 * The URL is matched by host and then by its first two path segments
 * (`/<class>/<slug>`), which is the shape both sites use for a project. A slug
 * is taken only when it is a single safe segment, and any deeper path is
 * ignored — so a link to a file inside a project still resolves to the project,
 * which is where the install lives, rather than being dropped.
 */
export function crossSourceLinkOf(url: string): CrossSourceLink | undefined {
	let parsed: URL
	try {
		parsed = new URL(url)
	} catch {
		return undefined
	}
	if (!/^https?:$/.test(parsed.protocol)) return undefined

	const host = parsed.hostname.toLowerCase().replace(/^www\./, '')
	const segments = parsed.pathname.split('/').filter(Boolean)

	if (host === 'curseforge.com') {
		if (segments[0] !== 'minecraft') return undefined
		const contentType = CURSEFORGE_PATHS[segments[1] ?? '']
		const projectId = segments[2]
		if (!contentType || !isSlug(projectId)) return undefined
		return { sourceId: 'curseforge', projectId, contentType }
	}

	if (host === 'modrinth.com') {
		const contentType = MODRINTH_PATHS[segments[0] ?? '']
		const projectId = segments[1]
		if (!contentType || !isSlug(projectId)) return undefined
		return { sourceId: MODRINTH_SOURCE_ID, projectId, contentType }
	}

	return undefined
}

/** A single path segment that could be a project slug. */
function isSlug(segment: string | undefined): segment is string {
	return Boolean(segment) && /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(segment as string)
}

/**
 * Whether the launcher can open the given link as a project page, which is the
 * case only when the source it points into is installed and loaded.
 */
export function canOpenCrossSourceLink(link: CrossSourceLink | undefined): boolean {
	if (!link) return false
	// Modrinth is built in, so it is always available; a plugin source has to be
	// installed and enabled, which is what a manifest in the installed list means.
	if (link.sourceId === MODRINTH_SOURCE_ID) return true
	return installedManifest(link.sourceId) !== null
}
