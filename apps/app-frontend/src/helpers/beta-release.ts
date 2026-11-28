/** A beta channel is a SemVer prerelease identifier, not a substring in an arbitrary tag. */
function betaVersion(tag: string): string[] | null {
	const match = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.exec(tag)
	if (!match) return null
	const identifiers = match[4].split('.')
	if (!identifiers.includes('beta') || identifiers.some((part) => /^\d+$/.test(part) && part.length > 1 && part.startsWith('0'))) return null
	return [...match.slice(1, 4), ...identifiers]
}

function compareNumber(a: string, b: string): number {
	return a.length - b.length || (a > b ? 1 : a < b ? -1 : 0)
}

/** Returns null for non-beta tags; otherwise compares full SemVer precedence without numeric overflow. */
export function compareBetaTags(a: string, b: string): number | null {
	const left = betaVersion(a)
	const right = betaVersion(b)
	if (!left || !right) return null
	for (let index = 0; index < 3; index++) {
		const order = compareNumber(left[index], right[index])
		if (order) return order
	}
	for (let index = 3; index < Math.max(left.length, right.length); index++) {
		if (left[index] === undefined) return -1
		if (right[index] === undefined) return 1
		const x = left[index], y = right[index]
		if (x === y) continue
		const xn = /^\d+$/.test(x), yn = /^\d+$/.test(y)
		return xn && yn ? compareNumber(x, y) : xn !== yn ? (xn ? -1 : 1) : x > y ? 1 : -1
	}
	return 0
}

export const isBetaTag = (tag: string): boolean => betaVersion(tag) !== null
