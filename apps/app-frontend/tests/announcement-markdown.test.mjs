import test from 'node:test'
import assert from 'node:assert/strict'
import { parseHtmlImage } from '../../../packages/utils/parse.ts'
import { renderAnnouncementMarkdown } from '../src/helpers/announcement-markdown.ts'

const src = 'https://github.com/user-attachments/assets/62fcde68-c985-410e-9465-ee2160c9b8cd'
const image = `<img width="2363" height="1343" alt="LumaShot-{99FBD4FA-3072-499D-80A3-2133920DE54F}" src="${src}" />`

test('GitHub image markup becomes an image rather than escaped text', () => {
	const html = renderAnnouncementMarkdown(image)
	assert.match(html, /<img\b/)
	assert.ok(html.includes(src))
	assert.ok(!html.includes('&lt;img'))
	assert.match(html, /width="2363"/)
	assert.match(html, /height="1343"/)
})

test('inline markup preserves surrounding text and quoted angle brackets', () => {
	const html = renderAnnouncementMarkdown(`before <img src='${src}' alt='a > b'> after`)
	assert.match(html, /<img\b/)
	assert.ok(html.includes('before'))
	assert.ok(html.includes('after'))
	assert.ok(html.includes('alt="a &gt; b"'))
})

test('event attributes, styles and srcset cannot reach rendered HTML', () => {
	const html = renderAnnouncementMarkdown(`<img src="${src}" onerror="alert(1)" style="position:fixed" srcset="file:///secret">`)
	assert.match(html, /<img\b/)
	assert.ok(!html.includes('onerror'))
	assert.ok(!html.includes('position:fixed'))
	assert.ok(!html.includes('srcset'))
})

test('unsafe URL schemes remain non-rendered markup', () => {
	for (const url of ['javascript:alert(1)', 'javascript&#58;alert(1)', 'file:///secret', 'asset://secret', 'data:image/svg+xml,secret']) {
		assert.doesNotMatch(renderAnnouncementMarkdown(`<img src="${url}">`), /<img\b/)
	}
})

test('other HTML and code examples stay inert', () => {
	assert.doesNotMatch(renderAnnouncementMarkdown('<script>alert(1)</script><iframe src="https://example.com"></iframe>'), /<(script|iframe)\b/)
	assert.doesNotMatch(renderAnnouncementMarkdown('```html\n' + image + '\n```'), /<img\b/)
	assert.doesNotMatch(renderAnnouncementMarkdown('`' + image + '`'), /<img\b/)
})

test('ambiguous, oversized or unfinished tags are rejected', () => {
	assert.equal(parseHtmlImage(`<img src="${src}" src="https://example.com/other">`), null)
	assert.equal(parseHtmlImage('<img src="unterminated>'), null)
	assert.equal(parseHtmlImage(`<img alt="${'a'.repeat(8192)}" src="${src}">`), null)
	assert.equal(parseHtmlImage('<iframe src="https://example.com">'), null)
	const parsed = parseHtmlImage(`<img src="${src}" width="99999999">`)
	assert.equal(parsed?.width, undefined)
})
