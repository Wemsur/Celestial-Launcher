// Plugin entry point. The launcher calls `activate(api)` once, after the app
// has rendered. Everything the plugin can do is on `api`; a capability the
// manifest did not declare throws when used.
import type { PluginHostApi } from '@celestial/plugin'

import SidebarCard from './SidebarCard.vue'

export async function activate(api: PluginHostApi): Promise<void> {
	api.styles.add(`
		.my-plugin-card {
			display: flex;
			flex-direction: column;
			gap: 4px;
			margin: 8px 16px;
			padding: 12px;
			border-radius: 10px;
			background: var(--color-brand);
			color: var(--color-accent-contrast);
			cursor: pointer;
		}
		.my-plugin-detail { font-size: 12px; opacity: 0.8; }
		.my-plugin-button {
			align-self: flex-start;
			margin-top: 4px;
			padding: 2px 8px;
			border-radius: 6px;
			border: none;
			cursor: pointer;
		}
	`)

	const opens = Number((await api.storage.get('opens')) ?? '0') + 1
	await api.storage.set('opens', String(opens))

	const pagePath = '/plugins/my-plugin'

	api.routes.add({
		path: pagePath,
		component: {
			name: 'MyPluginPage',
			setup() {
				const { h } = api.vue
				return () =>
					h('div', { class: 'p-6 flex flex-col gap-2' }, [
						h('h1', { class: 'm-0 text-2xl font-semibold' }, api.plugin.name),
						h('p', { class: 'm-0 text-secondary' }, `插件已启动 ${opens} 次。`),
					])
			},
		},
	})

	api.slots.add('sidebar.top', {
		id: 'card',
		component: SidebarCard,
		props: {
			pluginName: api.plugin.name,
			version: api.plugin.version,
			opens,
			onOpen: () => api.router.push(pagePath),
		},
	})

	api.log('activated; page at', pagePath)
}
