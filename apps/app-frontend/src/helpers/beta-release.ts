import { compareReleaseVersions, parseReleaseVersion } from './release-version'

/** A beta channel is a SemVer prerelease identifier, not a substring in an arbitrary tag. */
export const isBetaTag = (tag: string): boolean => parseReleaseVersion(tag)?.prerelease.includes('beta') ?? false

export function compareBetaTags(a: string, b: string): number | null {
	return isBetaTag(a) && isBetaTag(b) ? compareReleaseVersions(a, b) : null
}
