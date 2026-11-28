import test from 'node:test'
import assert from 'node:assert/strict'
import { compareReleaseVersions } from '../src/helpers/release-version.ts'

test('same core stable release is newer than beta or RC', () => {
	for (const current of ['3.5.0-beta', '3.5.0-beta.1', '3.5.0-beta.99', '3.5.0-rc.1']) {
		assert.equal(compareReleaseVersions('3.5.0', current), 1)
		assert.equal(compareReleaseVersions(current, '3.5.0'), -1)
	}
})

test('core versions take precedence and build metadata does not', () => {
	assert.equal(compareReleaseVersions('3.5.0', '3.4.0'), 1)
	assert.equal(compareReleaseVersions('3.5.0', '3.6.0-beta.1'), -1)
	assert.equal(compareReleaseVersions('v3.5.0+build.1', '3.5.0+build.2'), 0)
	assert.equal(compareReleaseVersions('3.5.0', '3.5.0'), 0)
})

test('prerelease identifiers follow SemVer precedence', () => {
	const ordered = ['3.5.0-alpha', '3.5.0-alpha.1', '3.5.0-alpha.beta', '3.5.0-beta', '3.5.0-beta.2', '3.5.0-beta.11', '3.5.0-rc.1', '3.5.0']
	for (let index = 1; index < ordered.length; index++) {
		assert.equal(compareReleaseVersions(ordered[index - 1], ordered[index]), -1)
	}
	assert.equal(compareReleaseVersions('3.5.0-beta.999999999999999999999', '3.5.0-beta.999999999999999999998'), 1)
})

test('invalid versions return null instead of NaN', () => {
	for (const invalid of ['3.5', '3.05.0', '3.5.0-beta.01', 'test.1', '3.5.0-']) {
		assert.equal(compareReleaseVersions(invalid, '3.5.0'), null)
	}
})
