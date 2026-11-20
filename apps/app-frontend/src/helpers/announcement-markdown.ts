import { configuredXss, md } from '@modrinth/utils/parse'

const IMAGE_BASE = 'https://raw.githubusercontent.com/Wemsur/celestial-announcement/main/'
const DOCUMENT_BASE = 'https://github.com/Wemsur/celestial-announcement/blob/main/'

export function announcementLink(raw: string | null, image = false): string | null {
	if (!raw || raw.startsWith('#')) return null
	try {
		const absolute = /^[a-z][a-z\d+.-]*:/i.test(raw) || raw.startsWith('//')
		const base = image ? IMAGE_BASE : DOCUMENT_BASE
		const url = new URL(raw.startsWith('//') ? `https:${raw}` : raw.replace(/^\/+/, ''), base)
		if ((image ? url.protocol !== 'https:' : !['http:', 'https:'].includes(url.protocol))
			|| url.username || url.password || (!absolute && !url.href.startsWith(base))) return null
		return url.href
	} catch {
		return null
	}
}

const parser = md({ html: false })
const imageRenderer = parser.renderer.rules.image
const linkRenderer = parser.renderer.rules.link_open
parser.renderer.rules.image = (tokens, index, options, env, self) => {
	const token = tokens[index]
	const src = announcementLink(token.attrGet('src'), true)
	if (!src) return parser.utils.escapeHtml(token.content)
	token.attrSet('src', src)
	return imageRenderer ? imageRenderer(tokens, index, options, env, self) : self.renderToken(tokens, index, options)
}
parser.renderer.rules.link_open = (tokens, index, options, env, self) => {
	const token = tokens[index]
	const href = announcementLink(token.attrGet('href'))
	if (href) token.attrSet('href', href)
	else token.attrs = token.attrs?.filter(([name]) => name !== 'href') ?? null
	return linkRenderer ? linkRenderer(tokens, index, options, env, self) : self.renderToken(tokens, index, options)
}

export function renderAnnouncementMarkdown(markdown: string): string {
	return configuredXss.process(parser.render(markdown))
}
