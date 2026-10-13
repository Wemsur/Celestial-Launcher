/**
 * Post-processing for a content source's rendered body.
 *
 * A source's own images are served from its CDN, and several of those CDNs
 * refuse a request that carries someone else's `Referer` — mcmod.cn's image
 * host answers 403 to anything but its own pages. The webview sends the
 * launcher's origin as the referrer by default, so those images fail to load
 * even though the URL itself is fine.
 *
 * `no-referrer` on each image fixes it, and it has to be stamped on here: the
 * body arrives as sanitised HTML rendered through `v-html`, so there is no
 * component to put the attribute on. Adding a fixed literal attribute to an
 * already-sanitised tree cannot introduce markup of its own.
 */
export function stripImageReferrer(html: string): string {
	if (!html || !html.includes('<img')) return html

	const doc = new DOMParser().parseFromString(html, 'text/html')
	for (const image of Array.from(doc.querySelectorAll('img'))) {
		image.setAttribute('referrerpolicy', 'no-referrer')
	}
	return doc.body.innerHTML
}
