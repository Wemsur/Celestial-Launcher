// Translating a rendered project description.
//
// The markdown is *not* translated as source text: machine translation reliably
// mangles link syntax, badge tables and code fences. Instead the already
// sanitised HTML is parsed, only its text nodes are replaced, and the tree is
// serialised again. Images, badges, code blocks and links survive untouched,
// and because only `textContent` is written the result cannot introduce markup.

/** Tags whose text is code or markup, never prose. */
const SKIPPED_TAGS = new Set(['CODE', 'PRE', 'KBD', 'SAMP', 'VAR', 'SCRIPT', 'STYLE', 'TEXTAREA'])

function isSkipped(node: Text): boolean {
	let parent = node.parentElement
	while (parent) {
		if (SKIPPED_TAGS.has(parent.tagName)) return true
		parent = parent.parentElement
	}
	return false
}

/** Nothing to translate in "1.20.1", "→" or "★★★". */
function hasWords(text: string): boolean {
	return /\p{L}{2}/u.test(text)
}

/**
 * Replaces the prose in `html` using `translate`, which returns the original
 * string for anything it has not translated yet. Call it again whenever more
 * translations arrive: it is a pure function of its inputs.
 */
export function translateHtml(html: string, translate: (text: string) => string): string {
	if (!html) return html

	const doc = new DOMParser().parseFromString(html, 'text/html')
	const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT)
	const nodes: Text[] = []

	while (walker.nextNode()) {
		nodes.push(walker.currentNode as Text)
	}

	let changed = false

	for (const node of nodes) {
		if (isSkipped(node)) continue

		// Inline markup splits a sentence across nodes, so the surrounding
		// whitespace has to be kept or neighbouring words glue together.
		const match = /^(\s*)([\s\S]*?)(\s*)$/.exec(node.data)
		if (!match) continue

		const [, leading, text, trailing] = match
		if (!text || !hasWords(text)) continue

		const translated = translate(text)
		if (translated === text) continue

		node.data = `${leading}${translated}${trailing}`
		changed = true
	}

	return changed ? doc.body.innerHTML : html
}
