import { computed, shallowRef } from 'vue'
import type { LocationQuery, RouteLocationNormalized, RouteLocationNormalizedLoaded } from 'vue-router'
import { useRoute, useRouter } from 'vue-router'

/**
 * Discover split view: the project detail page rendered as a nested route of
 * `/browse/:projectType`, so the discover list stays mounted in the left pane
 * while the detail fills the right pane.
 */
export const BROWSE_LIST_ROUTE_NAME = 'Discover content'
export const SPLIT_PROJECT_ROUTE_NAME = 'DiscoverSplitProject'
export const PLAIN_PROJECT_ROUTE_NAME = 'Project'

export const SPLIT_ROUTE_NAME_BY_SUBPAGE = {
	description: 'DiscoverSplitProjectDescription',
	versions: 'DiscoverSplitProjectVersions',
	version: 'DiscoverSplitProjectVersion',
	gallery: 'DiscoverSplitProjectGallery',
} as const

export const PLAIN_ROUTE_NAME_BY_SUBPAGE = {
	description: 'Description',
	versions: 'Versions',
	version: 'Version',
	gallery: 'Gallery',
} as const

export type ProjectSubpage = keyof typeof SPLIT_ROUTE_NAME_BY_SUBPAGE

/**
 * Query params that describe an install context. They are meaningful to both
 * panes, so they are the only ones carried across a split view toggle.
 */
const CONTEXT_QUERY_PARAMS = ['i', 'ai', 'shi', 'sid', 'wid', 'from'] as const

type RouteLike = RouteLocationNormalized | RouteLocationNormalizedLoaded

interface BrowseTarget {
	projectType: string
	query: LocationQuery
}

/** Last visited discover *list* route, used as the left pane when entering split view. */
const lastBrowseList = shallowRef<BrowseTarget | null>(null)
/** Fallback project type, published by the project page while it is open. */
const browseProjectTypeHint = shallowRef<string | null>(null)
let trackingRoute = false

export function isSplitProjectRoute(route: RouteLike): boolean {
	return route.matched.some((record) => record.name === SPLIT_PROJECT_ROUTE_NAME)
}

export function isPlainProjectRoute(route: RouteLike): boolean {
	return route.matched.some((record) => record.name === PLAIN_PROJECT_ROUTE_NAME)
}

/** True for both `/project/:id` and the split view's `/browse/:projectType/p/:id`. */
export function isProjectDetailRoute(route: RouteLike): boolean {
	return isPlainProjectRoute(route) || isSplitProjectRoute(route)
}

export function projectSubpageOf(route: RouteLike): ProjectSubpage {
	const name = typeof route.name === 'string' ? route.name : ''
	for (const subpage of Object.keys(SPLIT_ROUTE_NAME_BY_SUBPAGE) as ProjectSubpage[]) {
		if (name === SPLIT_ROUTE_NAME_BY_SUBPAGE[subpage] || name === PLAIN_ROUTE_NAME_BY_SUBPAGE[subpage]) {
			return subpage
		}
	}
	return 'description'
}

/** Path prefix every project detail link has to be built from. */
export function projectBasePathOf(route: RouteLike): string {
	const id = String(route.params.id ?? '')
	if (isSplitProjectRoute(route)) {
		return `/browse/${String(route.params.projectType ?? '')}/p/${id}`
	}
	return `/project/${id}`
}

/** `/browse/<type>`, keeping the open detail pane when the split view is active. */
export function browsePathFor(projectType: string, route: RouteLike): string {
	const base = `/browse/${projectType}`
	if (!isSplitProjectRoute(route)) return base
	const id = String(route.params.id ?? '')
	return id ? `${base}/p/${id}` : base
}

function stripQueryParam(query: LocationQuery, key: string): LocationQuery {
	const rest: LocationQuery = {}
	for (const [entryKey, value] of Object.entries(query)) {
		if (entryKey !== key) rest[entryKey] = value
	}
	return rest
}

function pickContextQuery(query: LocationQuery): LocationQuery {
	const picked: LocationQuery = {}
	for (const key of CONTEXT_QUERY_PARAMS) {
		if (query[key] !== undefined) picked[key] = query[key]
	}
	return picked
}

/** The list-only location of a discover route, ignoring any open detail pane. */
export function browseListLocation(route: RouteLike): { path: string; query: LocationQuery } {
	return {
		path: `/browse/${String(route.params.projectType ?? '')}`,
		query: stripQueryParam(route.query, 'b'),
	}
}

export function browseListFullPath(route: RouteLike): string {
	const location = browseListLocation(route)
	const params = new URLSearchParams()
	for (const [key, value] of Object.entries(location.query)) {
		if (Array.isArray(value)) {
			for (const entry of value) {
				if (entry !== null && entry !== '') params.append(key, String(entry))
			}
		} else if (value !== null && value !== undefined && value !== '') {
			params.append(key, String(value))
		}
	}
	const queryString = params.toString()
	return queryString ? `${location.path}?${queryString}` : location.path
}

/** Parses a `?b=/browse/mod?...` back-link into a left pane target. */
function parseBrowseTarget(raw: string): BrowseTarget | null {
	if (!raw.startsWith('/browse/')) return null

	const [pathPart, queryPart] = raw.split('?')
	const projectType = pathPart.split('/').filter(Boolean)[1]
	if (!projectType) return null

	const query: LocationQuery = {}
	if (queryPart) {
		for (const [key, value] of new URLSearchParams(queryPart)) {
			if (key === 'b') continue
			const existing = query[key]
			if (existing === undefined) {
				query[key] = value
			} else if (Array.isArray(existing)) {
				existing.push(value)
			} else {
				query[key] = [existing, value]
			}
		}
	}

	return { projectType, query }
}

function rememberBrowseList(route: RouteLike) {
	if (route.name !== BROWSE_LIST_ROUTE_NAME) return
	lastBrowseList.value = {
		projectType: String(route.params.projectType ?? 'mod'),
		query: { ...route.query },
	}
}

export function setBrowseProjectTypeHint(projectType: string | null | undefined) {
	if (projectType) browseProjectTypeHint.value = projectType
}

export function useSplitView() {
	const route = useRoute()
	const router = useRouter()

	if (!trackingRoute) {
		trackingRoute = true
		rememberBrowseList(router.currentRoute.value)
		router.afterEach((to) => rememberBrowseList(to))
	}

	const splitViewActive = computed(() => isSplitProjectRoute(route))
	const canUseSplitView = computed(() => splitViewActive.value || isPlainProjectRoute(route))

	function resolveBrowseTarget(current: RouteLike): BrowseTarget {
		const backLink = current.query.b
		const fromBackLink = typeof backLink === 'string' ? parseBrowseTarget(backLink) : null
		if (fromBackLink) return fromBackLink
		if (lastBrowseList.value) return lastBrowseList.value
		return { projectType: browseProjectTypeHint.value ?? 'mod', query: {} }
	}

	function enterSplitView() {
		const current = router.currentRoute.value
		const id = String(current.params.id ?? '')
		if (!isPlainProjectRoute(current) || !id) return

		const target = resolveBrowseTarget(current)
		const subpage = projectSubpageOf(current)
		const params: Record<string, string> = { projectType: target.projectType, id }
		if (subpage === 'version') params.version = String(current.params.version ?? '')

		void router.push({
			name: SPLIT_ROUTE_NAME_BY_SUBPAGE[subpage],
			params,
			query: { ...target.query, ...pickContextQuery(current.query) },
		})
	}

	function exitSplitView() {
		const current = router.currentRoute.value
		const id = String(current.params.id ?? '')
		if (!isSplitProjectRoute(current) || !id) return

		const subpage = projectSubpageOf(current)
		const params: Record<string, string> = { id }
		if (subpage === 'version') params.version = String(current.params.version ?? '')

		void router.push({
			name: PLAIN_ROUTE_NAME_BY_SUBPAGE[subpage],
			params,
			query: { ...pickContextQuery(current.query), b: browseListFullPath(current) },
		})
	}

	function toggleSplitView() {
		if (splitViewActive.value) {
			exitSplitView()
		} else {
			enterSplitView()
		}
	}

	return {
		splitViewActive,
		canUseSplitView,
		enterSplitView,
		exitSplitView,
		toggleSplitView,
	}
}
