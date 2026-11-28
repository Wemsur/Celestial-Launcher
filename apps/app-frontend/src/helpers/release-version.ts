interface ReleaseVersion {
	core: string[]
	prerelease: string[]
}

export function parseReleaseVersion(tag: string): ReleaseVersion | null {
	const match = /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.exec(tag)
	if (!match) return null
	const prerelease = match[4]?.split('.') ?? []
	if (prerelease.some((part) => /^\d+$/.test(part) && part.length > 1 && part.startsWith('0'))) return null
	return { core: match.slice(1, 4), prerelease }
}

function compareNumber(a: string, b: string): number {
	return a.length - b.length || (a > b ? 1 : a < b ? -1 : 0)
}

/** SemVer precedence; build metadata is ignored and invalid inputs never become NaN. */
export function compareReleaseVersions(a: string, b: string): number | null {
	const left = parseReleaseVersion(a)
	const right = parseReleaseVersion(b)
	if (!left || !right) return null
	for (let index = 0; index < 3; index++) {
		const order = compareNumber(left.core[index], right.core[index])
		if (order) return Math.sign(order)
	}
	if (!left.prerelease.length || !right.prerelease.length) {
		return left.prerelease.length === right.prerelease.length ? 0 : left.prerelease.length ? -1 : 1
	}
	for (let index = 0; index < Math.max(left.prerelease.length, right.prerelease.length); index++) {
		const x = left.prerelease[index], y = right.prerelease[index]
		if (x === undefined) return -1
		if (y === undefined) return 1
		if (x === y) continue
		const xn = /^\d+$/.test(x), yn = /^\d+$/.test(y)
		return xn && yn ? Math.sign(compareNumber(x, y)) : xn !== yn ? (xn ? -1 : 1) : x > y ? 1 : -1
	}
	return 0
}
