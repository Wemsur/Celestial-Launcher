import test from 'node:test'
import assert from 'node:assert/strict'
import { compareBetaTags, isBetaTag } from '../src/helpers/beta-release.ts'

test('beta channel excludes stable, RC and arbitrary test tags', () => {
	assert.equal(isBetaTag('v3.5.0-beta.4'), true)
	assert.equal(isBetaTag('3.5.0-beta+build.1'), true)
	for (const tag of ['v3.5.0', 'v3.5.0-rc.1', 'distribution-test-123-1', 'v3.5.0-betatest.1', 'v3.5.0-beta.04']) {
		assert.equal(isBetaTag(tag), false)
	}
})

test('beta numbers use numeric SemVer ordering, not lexicographic order', () => {
	assert.ok(compareBetaTags('v3.5.0-beta.10', 'v3.5.0-beta.9') > 0)
	assert.ok(compareBetaTags('v3.6.0-beta.1', 'v3.5.0-beta.100') > 0)
	assert.ok(compareBetaTags('v3.5.0-beta.1', 'v3.5.0-beta') > 0)
	assert.equal(compareBetaTags('v3.5.0-beta.4+build.1', 'v3.5.0-beta.4+build.2'), 0)
	assert.equal(compareBetaTags('v3.5.0', 'v3.5.0-beta.4'), null)
})
