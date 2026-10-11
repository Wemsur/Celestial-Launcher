import { createRouter, createWebHistory } from 'vue-router'

import * as Pages from '@/pages'
import * as Library from '@/pages/library'
import * as Project from '@/pages/project'

/**
 * The tabs of a content source's project page.
 *
 * Declared twice with distinct names rather than shared: Vue Router binds a
 * child record to the parent that registered it, so handing the same array to
 * two parents leaves the second one unable to match its own grandchildren —
 * the tabs navigate but render nothing. The Modrinth project routes avoid this
 * only because they happen to declare each set separately.
 *
 * The project id is a single segment because a source's id is passed as one —
 * a GitHub entry's `owner/repo` is percent-encoded into the URL, so the child
 * segments below stay unambiguous.
 */
const splitSourceProjectChildren = [
	{
		path: '',
		name: 'DiscoverSplitSourceProjectDescription',
		component: () => import('@/pages/source-project/Description.vue'),
	},
	{
		path: 'versions',
		name: 'DiscoverSplitSourceProjectVersions',
		component: () => import('@/pages/source-project/Versions.vue'),
	},
	{
		path: 'version/:version',
		name: 'DiscoverSplitSourceProjectVersion',
		component: () => import('@/pages/source-project/Version.vue'),
	},
	{
		path: 'gallery',
		name: 'DiscoverSplitSourceProjectGallery',
		component: () => import('@/pages/source-project/Gallery.vue'),
	},
]

const plainSourceProjectChildren = [
	{
		path: '',
		name: 'SourceProjectDescription',
		component: () => import('@/pages/source-project/Description.vue'),
	},
	{
		path: 'versions',
		name: 'SourceProjectVersions',
		component: () => import('@/pages/source-project/Versions.vue'),
	},
	{
		path: 'version/:version',
		name: 'SourceProjectVersion',
		component: () => import('@/pages/source-project/Version.vue'),
	},
	{
		path: 'gallery',
		name: 'SourceProjectGallery',
		component: () => import('@/pages/source-project/Gallery.vue'),
	},
]

/**
 * Loads each page when its route is visited.
 */
export default new createRouter({
	history: createWebHistory(),
	routes: [
		{
			path: '/',
			name: 'Home',
			component: () => import('@/pages/Index.vue'),
		},
		{
			path: '/hosting/manage/',
			name: 'Servers',
			component: () => import('@/pages/Servers.vue'),
		},
		{
			path: '/hosting/manage/:id',
			name: 'ServerManage',
			component: () => import('@/pages/hosting/manage/Index.vue'),
			children: [
				{
					path: '',
					name: 'ServerManageOverview',
					component: () => import('@/pages/hosting/manage/Overview.vue'),
				},
				{
					path: 'content',
					name: 'ServerManageContent',
					component: () => import('@/pages/hosting/manage/Content.vue'),
				},
				{
					path: 'files',
					name: 'ServerManageFiles',
					component: () => import('@/pages/hosting/manage/Files.vue'),
				},
				{
					path: 'backups',
					name: 'ServerManageBackups',
					component: () => import('@/pages/hosting/manage/Backups.vue'),
				},
				{
					path: 'access',
					name: 'ServerManageAccess',
					component: () => import('@/pages/hosting/manage/Access.vue'),
				},
			],
		},
		{
			path: '/browse/:projectType',
			name: 'Discover content',
			component: Pages.Browse,
			// Split view: the project page renders inside Browse's right pane, so the
			// discover list keeps its state while a project is open.
			children: [
				{
					path: 'p/:id',
					name: 'DiscoverSplitProject',
					component: Project.Index,
					props: true,
					children: [
						{
							path: '',
							name: 'DiscoverSplitProjectDescription',
							component: Project.Description,
						},
						{
							path: 'versions',
							name: 'DiscoverSplitProjectVersions',
							component: Project.Versions,
						},
						{
							path: 'version/:version',
							name: 'DiscoverSplitProjectVersion',
							component: Project.Version,
							props: true,
						},
						{
							path: 'gallery',
							name: 'DiscoverSplitProjectGallery',
							component: Project.Gallery,
						},
					],
				},
				{
					// A project from a content source, shown inside the discover split
					// pane the same way a Modrinth one is.
					path: 's/:sourceId/:projectId',
					name: 'DiscoverSplitSourceProject',
					component: () => import('@/pages/source-project/Index.vue'),
					children: splitSourceProjectChildren,
				},
			],
		},
		{
			path: '/skins',
			name: 'Skin selector',
			component: () => import('@/pages/Skins.vue'),
		},
		{
			path: '/screenshots',
			name: 'Screenshots',
			component: () => import('@/pages/Screenshots.vue'),
		},
		{
			path: '/user/:user/:projectType?',
			name: 'User',
			component: () => import('@/pages/User.vue'),
		},
		{
			path: '/library',
			name: 'Library',
			component: Library.Index,
			children: [
				{
					path: '',
					name: 'Overview',
					component: Library.Overview,
				},
				{
					path: 'downloaded',
					name: 'Downloaded',
					component: Library.Downloaded,
				},
				{
					path: 'modpacks',
					name: 'Modpacks',
					component: Library.Modpacks,
				},
				{
					path: 'servers',
					name: 'LibraryServers',
					component: Library.Servers,
				},
				{
					path: 'custom',
					name: 'Custom',
					component: Library.Custom,
				},
				{
					path: 'lib/:libPath(.*)',
					name: 'LibraryByPath',
					component: Library.Overview,
				},
			],
		},
		{
			path: '/:projectType(mod|plugin|datapack|resourcepack|shader|modpack)/:id/:rest(.*)*',
			redirect: (to) => {
				const rest = to.params.rest ? `/${[].concat(to.params.rest).join('/')}` : ''
				return `/project/${to.params.id}${rest}${to.hash}`
			},
		},
		{
			// A project from a content source, at full width (no split view).
			path: '/source/:sourceId/:projectId',
			name: 'SourceProject',
			component: () => import('@/pages/source-project/Index.vue'),
			children: plainSourceProjectChildren,
		},
		{
			path: '/project/:id',
			name: 'Project',
			component: () => import('@/pages/project/Index.vue'),
			props: true,
			children: [
				{
					path: '',
					name: 'Description',
					component: () => import('@/pages/project/Description.vue'),
				},
				{
					path: 'versions',
					name: 'Versions',
					component: () => import('@/pages/project/Versions.vue'),
				},
				{
					path: 'version/:version',
					name: 'Version',
					component: () => import('@/pages/project/Version.vue'),
					props: true,
				},
				{
					path: 'gallery',
					name: 'Gallery',
					component: () => import('@/pages/project/Gallery.vue'),
				},
			],
		},
		{
			path: '/instance/:id',
			name: 'Instance',
			component: () => import('@/pages/instance/layout.vue'),
			children: [
				{
					path: 'worlds',
					name: 'InstanceWorlds',
					component: () => import('@/pages/instance/worlds/index.vue'),
				},
				{
					path: 'share',
					name: 'InstanceShare',
					component: () => import('@/pages/instance/share/index.vue'),
				},
				{
					path: '',
					name: 'InstanceContent',
					component: () => import('@/pages/instance/content/index.vue'),
				},
				{
					path: 'projects/:type',
					name: 'InstanceContentFilter',
					component: () => import('@/pages/instance/content/index.vue'),
				},
				{
					path: 'files',
					name: 'InstanceFiles',
					component: () => import('@/pages/instance/files/index.vue'),
				},
				{
					path: 'screenshots',
					name: 'InstanceScreenshots',
					component: () => import('@/pages/instance/screenshots/index.vue'),
				},
				{
					path: 'logs',
					name: 'InstanceLogs',
					component: () => import('@/pages/instance/logs/index.vue'),
					meta: {
						renderMode: 'fixed',
					},
				},
			],
		},
	],
	linkActiveClass: 'router-link-active',
	linkExactActiveClass: 'router-link-exact-active',
	scrollBehavior(to, from) {
		// Both the Modrinth and the content-source detail panes are split-view
		// routes with their own scroll container; the source ones were missed,
		// so a source project opened at the previous project's scroll offset.
		const isSplitProject = (route) =>
			route.matched.some(
				(record) =>
					record.name === 'DiscoverSplitProject' ||
					record.name === 'DiscoverSplitSourceProject',
			)
		if (isSplitProject(to)) {
			// The detail pane is its own scroll container; reset it, never the list.
			document.querySelector('.browse-detail-pane')?.scrollTo(0, 0)
			if (isSplitProject(from)) return
		}
		if (to.path === from.path || to.name === 'Home') return
		// Sometimes Vue's scroll behavior is not working as expected, so we need to manually scroll to top (especially on Linux)
		document.querySelector('.app-viewport')?.scrollTo(0, 0)
		return {
			el: '.app-viewport',
			top: 0,
		}
	},
})
