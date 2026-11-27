import { createHash } from 'node:crypto'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
	apiClient, inspectFile, uploadTarget, uploadFile, verifyAssets, withGit, pushTag, summary, gitDiagnostic,
} from './publish-gitcode.mjs'

const API = 'https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher'
const WEB = 'https://gitcode.com/Wemsur/Celestial-Launcher'
const MAX_BYTES = 64 * 1024

function check(condition, message) {
	if (!condition) throw new Error(message)
}

function allowedDownload(url) {
	if (url.protocol !== 'https:' || url.username || url.password || url.hash || (url.port && url.port !== '443')) return false
	if (url.hostname === 'api.gitcode.com') return url.pathname.startsWith('/api/v5/repos/Wemsur/Celestial-Launcher/releases/') && url.pathname.endsWith('/download')
	if (url.hostname === 'gitcode.com') return url.pathname.startsWith('/Wemsur/Celestial-Launcher/')
	return /^(?:gitcode|atomgit)(?:-[a-z0-9-]+)?\.obs\.[a-z0-9-]+\.myhuaweicloud\.com$/.test(url.hostname)
}

async function verifyAnonymousDownload(tag, file) {
	const deadline = AbortSignal.timeout(60_000)
	let url = new URL(`${API}/releases/${encodeURIComponent(tag)}/attach_files/${encodeURIComponent(file.name)}/download`)
	for (let hop = 0; hop <= 5; hop++) {
		check(allowedDownload(url), 'Anonymous download redirected outside the client allowlist; do not broaden it without checking the actual host')
		console.log(`Anonymous download hop ${hop}: ${url.origin}${url.pathname}`)
		const response = await fetch(url, { redirect: 'manual', signal: deadline })
		console.log(`Anonymous download HTTP ${response.status}`)
		if ([301, 302, 303, 307, 308].includes(response.status)) {
			const location = response.headers.get('location')
			await response.body?.cancel()
			check(location, 'Anonymous redirect has no location')
			url = new URL(location, url)
			continue
		}
		check(response.status === 200, `Anonymous download failed (HTTP ${response.status})`)
		const hash = createHash('sha256')
		let bytes = 0
		const reader = response.body?.getReader()
		check(reader, 'Anonymous download has no body')
		try {
			for (;;) {
				const { done, value } = await reader.read()
				if (done) break
				bytes += value.length
				check(bytes <= MAX_BYTES, 'Anonymous download exceeded the test file limit')
				hash.update(value)
			}
		} finally { await reader.cancel() }
		check(bytes === file.size && hash.digest('hex') === file.sha256, 'Anonymous download size or SHA256 mismatch')
		console.log(`Anonymous download verified: ${bytes} bytes; SHA256 ${file.sha256}`)
		return
	}
	throw new Error('Too many anonymous download redirects')
}

async function main() {
	check(Number(process.versions.node.split('.')[0]) >= 22, 'Node.js 22 or newer is required')
	check(process.env.GITHUB_EVENT_NAME === 'workflow_dispatch', 'Only manual workflow runs are supported')
	const run = process.env.GITHUB_RUN_ID
	const attempt = process.env.GITHUB_RUN_ATTEMPT
	check(/^[1-9]\d*$/.test(run ?? '') && /^[1-9]\d*$/.test(attempt ?? ''), 'Invalid GitHub run identity')
	const token = process.env.GITCODE_TOKEN
	check(token && token === token.trim() && !/[\x00-\x1f\x7f]/.test(token), 'Missing or invalid GITCODE_TOKEN')
	check(process.env.GITCODE_USERNAME && !/[\x00-\x20\x7f]/.test(process.env.GITCODE_USERNAME), 'Missing or invalid GITCODE_USERNAME')
	const tag = `distribution-test-${run}-${attempt}`
	const link = `${WEB}/releases/tag/${tag}`
	const api = apiClient(token)
	const directory = await mkdtemp(join(tmpdir(), 'celestial-distribution-test-'))
	try {
		console.log(`Step 1: Generate public test file for ${tag}`)
		const name = 'connectivity-test.txt'
		const filePath = join(directory, name)
		await writeFile(filePath, `Celestial GitCode distribution smoke test\nRun: ${run}\nAttempt: ${attempt}\nTime: ${new Date().toISOString()}\n`, { flag: 'wx' })
		const file = { ...await inspectFile(filePath, MAX_BYTES), name }
		console.log('Step 2: Verify repository API access and release absence')
		await api('')
		const releasePath = `/releases/tags/${tag}`
		check(await api(releasePath, { allowMissing: true }) === null, 'Test release already exists; refusing to overwrite')
		console.log('Step 3: Verify Git access and push a dedicated test tag')
		let commit
		await withGit(async (git) => {
			commit = await git('read smoke-test checkout', ['rev-parse', 'HEAD'])
			check(/^[0-9a-f]{40}$/.test(commit), 'Invalid checkout commit')
			await pushTag(git, tag, commit)
		})
		console.log('Step 4: Create public prerelease (never latest)')
		await api('/releases', { method: 'POST', body: {
			tag_name: tag, name: `Distribution test ${run}/${attempt}`,
			body: '公开传输测试，仅包含小文本文件，不是启动器版本。测试后可手动删除此预发布及对应 distribution-test tag。',
			target_commitish: commit, release_status: 'pre',
		} })
		console.log('Step 5: Obtain OBS upload target and upload text file')
		const target = uploadTarget(await api(`/releases/${tag}/upload_url?file_name=${name}`), token)
		console.log(`Upload host: ${target.url.hostname}`)
		await uploadFile(target, file)
		console.log('Step 6: Verify prerelease state and registered attachment')
		const release = await api(releasePath)
		check(release.tag_name === tag && release.release_status === 'pre', 'Unexpected test release tag or status')
		verifyAssets(release, [file])
		console.log('Step 7: Download without any token and compare SHA256')
		await verifyAnonymousDownload(tag, file)
		await summary(`### GitCode 传输测试通过\n\n[测试预发布](${link})\n\n文件：${name}（${file.size} bytes），Git 推送、预发布创建、附件上传和匿名下载 SHA256 均通过。\n\n这不是安装包验签测试。可手动删除该预发布，并单独删除 tag \`${tag}\`；不要推广为 latest。脚本不会自动删除远端内容。`)
		console.log(`Test passed: ${link}; test tag: ${tag}`)
	} catch (error) {
		await summary(`### GitCode 传输测试未完成\n\n[检查测试预发布](${link})；对应 tag：\`${tag}\`。可能已创建 tag 或部分预发布，请核查后分别清理，不要推广。详细失败步骤和脱敏错误见日志。`)
		throw error
	} finally { await rm(directory, { recursive: true, force: true }) }
}

main().catch((error) => {
	console.error(`GitCode smoke test failed:\n${gitDiagnostic(String(error.message)).slice(0, 16_000)}`)
	process.exitCode = 1
})
