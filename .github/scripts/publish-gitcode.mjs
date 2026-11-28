import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { constants } from 'node:fs'
import { appendFile, lstat, mkdtemp, open, opendir, rm, writeFile } from 'node:fs/promises'
import https from 'node:https'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { Transform } from 'node:stream'
import { pipeline } from 'node:stream/promises'

const REPOSITORY = 'https://gitcode.com/Wemsur/Celestial-Launcher.git'
const WEB = 'https://gitcode.com/Wemsur/Celestial-Launcher'
const API = 'https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher'
const MAX_FILES = 64
const MAX_FILE_BYTES = 4 * 1024 ** 3
const MAX_TOTAL_BYTES = 16 * 1024 ** 3
const MAX_JSON_BYTES = 2 * 1024 ** 2
const API_TIMEOUT_MS = 60_000
const UPLOAD_TIMEOUT_MS = 30 * 60_000
const GIT_TIMEOUT_MS = 15 * 60_000
const OBS_HOST = /^[a-z0-9][a-z0-9.-]*\.obs\.[a-z0-9-]+\.myhuaweicloud\.com$/

function required(name) {
	const value = process.env[name]
	if (!value || value !== value.trim() || /[\x00-\x1f\x7f]/.test(value)) {
		throw new Error(`Missing or invalid environment variable: ${name}`)
	}
	return value
}

function check(condition, message) {
	if (!condition) throw new Error(message)
}

function diagnosticCode(value) {
	return /^[A-Za-z0-9_.-]{1,80}$/.test(String(value ?? '')) ? String(value) : 'unknown'
}

function apiErrorCode(data) {
	return `code=${diagnosticCode(data?.error_code)}; name=${diagnosticCode(data?.error_code_name)}`
}

/** Read bounded JSON without exposing server bodies or credential-bearing errors. */
async function requestJson(url, { method = 'GET', headers = {}, body, timeout = API_TIMEOUT_MS, upload } = {}) {
	const started = Date.now()
	const endpoint = url.hostname === 'api.gitcode.com' ? `${url.origin}${url.pathname}` : `${url.origin}/[object]`
	console.log(`HTTP request: ${method} ${endpoint}`)
	return new Promise((accept, reject) => {
		let timer
		let settled = false
		const finish = (error, result) => {
			if (settled) return
			settled = true
			clearTimeout(timer)
			if (error) reject(error)
			else accept(result)
		}
		const request = https.request(url, { method, headers }, (response) => {
			const chunks = []
			console.log(`HTTP response: ${method} ${endpoint}; status=${response.statusCode}; elapsedMs=${Date.now() - started}`)
			let bytes = 0
			response.on('data', (chunk) => {
				bytes += chunk.length
				if (bytes > MAX_JSON_BYTES) {
					request.destroy()
					response.destroy()
					finish(new Error('Response exceeded the allowed size'))
				} else chunks.push(chunk)
			})
			response.on('error', (error) => finish(new Error(`Response interrupted (${method} ${endpoint}); code=${diagnosticCode(error.code)}`)))
			response.on('end', () => {
				const status = response.statusCode
				if (status >= 300 && status < 400) {
					finish(new Error('Redirect refused'))
					return
				}
				let data
				try {
					const text = Buffer.concat(chunks).toString('utf8')
					data = text ? JSON.parse(text) : null
				} catch {
					if (method !== 'PUT' || status < 200 || status >= 300) {
						finish(new Error(`Invalid JSON response (HTTP ${status})`))
						return
					}
				}
				if (status >= 200 && status < 300 && data && typeof data === 'object' && Object.hasOwn(data, 'error_code') &&
					data.error_code !== 0 && data.error_code !== '0' && data.error_code !== null) {
					finish(new Error(`Remote API reported an error (HTTP ${status}); ${apiErrorCode(data)}`))
					return
				}
				finish(null, { status, data })
			})
		})
		request.on('error', (error) => finish(new Error(`Network request failed (${method} ${endpoint}); code=${diagnosticCode(error.code)}; cause=${diagnosticCode(error.cause?.code)}`)))
		timer = setTimeout(() => {
			request.destroy()
			finish(new Error('Network request timed out'))
		}, timeout)
		if (upload) {
			upload(request).catch((error) => {
				request.destroy()
				finish(new Error(`Upload stream failed or artifact changed; code=${diagnosticCode(error.code)}; details=${gitDiagnostic(String(error.message)).slice(0, 2000)}`))
			})
		} else request.end(body)
	})
}

function apiClient(token) {
	return async (path, { method = 'GET', body, allowMissing = false } = {}) => {
		const result = await requestJson(new URL(`${API}${path}`), {
			method,
			headers: {
				Authorization: `Bearer ${token}`,
				Accept: 'application/json',
				...(body ? { 'Content-Type': 'application/json' } : {}),
			},
			body: body ? JSON.stringify(body) : undefined,
		})
		if (allowMissing && (result.status === 404 || (result.status === 400 &&
			result.data?.error_code === 400 && result.data?.error_code_name === 'BAD_REQUEST' &&
			result.data?.error_message === 'Release not found'))) return null
		check(result.status >= 200 && result.status < 300, `GitCode API request failed (HTTP ${result.status}); ${apiErrorCode(result.data)}`)
		check(result.data && typeof result.data === 'object', 'GitCode API returned no object')
		return result.data
	}
}

async function inspectFile(path, maxBytes) {
	const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW)
	try {
		const stat = await handle.stat()
		check(stat.isFile() && stat.size > 0 && stat.size <= maxBytes, 'Invalid artifact type or size')
		const sha256 = createHash('sha256')
		const sha512 = createHash('sha512')
		let bytes = 0
		for await (const chunk of handle.createReadStream({ autoClose: false })) {
			bytes += chunk.length
			check(bytes <= maxBytes, 'Artifact exceeded the allowed size')
			sha256.update(chunk)
			sha512.update(chunk)
		}
		check(bytes === stat.size, 'Artifact changed during preflight')
		return { path, size: bytes, sha256: sha256.digest('hex'), sha512: sha512.digest('hex'), stat }
	} finally {
		await handle.close()
	}
}

/** Accept only the flat, signed installer names produced by this workflow. */
async function preflight(directory, version) {
	check((await lstat(directory)).isDirectory(), 'Artifact input must be a real directory')
	const names = []
	for await (const entry of await opendir(directory)) {
		check(names.length < MAX_FILES && entry.isFile(), 'Artifact input contains too many files or a non-regular file')
		names.push(entry.name)
	}
	check(names.length >= 2, 'Expected at least one installer/signature pair')
	const installerNames = new Set()
	for (const platform of ['windows-x64', 'windows-arm64', 'linux-x64', 'linux-arm64', 'macos-universal']) {
		const extensions = platform.startsWith('windows') ? ['-setup.exe', '.msi'] :
			platform.startsWith('linux') ? ['.deb', '.rpm', '.AppImage'] : ['.dmg', '.app.tar.gz']
		for (const extension of extensions) installerNames.add(`CelestialLauncher-${version}-${platform}${extension}`)
	}
	const present = new Set(names)
	const files = []
	let total = 0
	for (const name of names.sort()) {
		const signature = name.endsWith('.sig')
		const installer = signature ? name.slice(0, -4) : name
		check(installerNames.has(installer), 'Unexpected artifact name')
		check(present.has(installer) && present.has(`${installer}.sig`), 'Every installer needs its matching .sig file')
		const file = await inspectFile(join(directory, name), signature ? 64 * 1024 : MAX_FILE_BYTES)
		total += file.size
		check(total <= MAX_TOTAL_BYTES, 'Artifact input exceeded the total size limit')
		files.push({ ...file, name })
	}
	return files
}

/** Sanitize complete diagnostics before truncating, so a boundary cannot expose a partial token. */
function gitDiagnostic(text) {
	const token = process.env.GITCODE_TOKEN || ''
	const username = process.env.GITCODE_USERNAME || ''
	const secrets = [
		token, username,
		token && encodeURIComponent(token),
		username && encodeURIComponent(username),
		token && Buffer.from(`${username}:${token}`).toString('base64'),
	].filter(Boolean).sort((a, b) => b.length - a.length)
	for (const secret of secrets) text = text.split(secret).join('[REDACTED]')
	return text
		.replace(/https?:\/\/[^\s<>"']+/gi, (raw) => {
			try {
				const url = new URL(raw)
				return `${url.protocol}//${url.host}${url.pathname}${url.search ? '?[REDACTED]' : ''}${url.hash ? '#[REDACTED]' : ''}`
			} catch { return '[REDACTED URL]' }
		})
		.replace(/\b(?:authorization|proxy-authorization)\s*:[^\r\n]*/gi, '[REDACTED AUTHORIZATION]')
		.replace(/\b(?:Bearer|Basic)\s+[^\s]+/gi, '[REDACTED AUTHORIZATION]')
		.replace(/\b(access_token|private_token|password|passwd|token)\s*[=:]\s*[^\s&]+/gi, '$1=[REDACTED]')
		.replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '')
		.split(/\r?\n/).map((line) => `  | ${line}`).join('\n')
}

/** Git uses argv and a temporary askpass program; credentials never enter URLs or config. */
async function withGit(callback) {
	const directory = await mkdtemp(join(tmpdir(), 'celestial-gitcode-'))
	const askpass = join(directory, 'askpass.mjs')
	try {
		await writeFile(askpass, '#!/usr/bin/env node\nconst prompt = process.argv[2] || ""\nif (/username/i.test(prompt)) process.stdout.write(process.env.GITCODE_USERNAME + "\\n")\nelse if (/password/i.test(prompt)) process.stdout.write(process.env.GITCODE_TOKEN + "\\n")\nelse process.exit(1)\n', { mode: 0o700 })
		const env = { ...process.env, GIT_ASKPASS: askpass, GIT_TERMINAL_PROMPT: '0' }
		for (const key of Object.keys(env)) {
			if (key.startsWith('GIT_TRACE') || key === 'GIT_CURL_VERBOSE') delete env[key]
		}
		const git = (step, args) => new Promise((accept, reject) => {
			console.log(`GitCode Git step: ${step}`)
			const started = Date.now()
			const child = spawn('git', ['-c', 'http.followRedirects=false', '-c', 'credential.helper=',
				'-c', 'http.extraHeader=', ...args], { env, shell: false, stdio: ['ignore', 'pipe', 'pipe'] })
			const stdout = []
			const stderr = []
			let bytes = 0
			let exceeded = false
			let timedOut = false
			const collect = (chunk, target) => {
				bytes += chunk.length
				if (bytes > MAX_JSON_BYTES) {
					exceeded = true
					child.kill('SIGKILL')
				} else target.push(chunk)
			}
			child.stdout.on('data', (chunk) => collect(chunk, stdout))
			child.stderr.on('data', (chunk) => collect(chunk, stderr))
			const timer = setTimeout(() => {
				timedOut = true
				child.kill('SIGKILL')
			}, GIT_TIMEOUT_MS)
			child.on('error', (error) => {
				clearTimeout(timer)
				reject(new Error(`Cannot start Git (${step}); code=${error.code || 'unknown'}`))
			})
			child.on('close', (code, signal) => {
				clearTimeout(timer)
				if (code !== 0 || exceeded || timedOut) {
					const details = exceeded
						? '  | Output exceeded the capture limit; partial diagnostics withheld to avoid exposing truncated secrets.'
						: gitDiagnostic(Buffer.concat(stderr.length ? stderr : stdout).toString('utf8')).slice(0, 12_000)
					reject(new Error(`Git operation failed (${step}); exit=${code}; signal=${signal || 'none'}; timeout=${timedOut}; outputLimit=${exceeded}; elapsedMs=${Date.now() - started}\n${details || '  | No diagnostic output'}`))
				} else {
					console.log(`GitCode Git step completed: ${step} (${Date.now() - started}ms)`)
					accept(Buffer.concat(stdout).toString('utf8').trim())
				}
			})
		})
		return await callback(git)
	} finally {
		await rm(directory, { recursive: true, force: true })
	}
}

async function pushTag(git, tag, commit) {
	const ref = `refs/tags/${tag}`
	await git('validate tag name', ['check-ref-format', ref])
	check(await git('read checkout commit', ['rev-parse', 'HEAD']) === commit, 'Checkout does not match RELEASE_COMMIT')
	check(await git('check full source history', ['rev-parse', '--is-shallow-repository']) === 'false', 'Full source history is required')
	const refs = await git('query GitCode tag', ['ls-remote', '--tags', REPOSITORY, ref, `${ref}^{}`])
	if (refs) {
		const entries = new Map(refs.split('\n').map((line) => {
			const [sha, name] = line.split(/\s+/)
			check(/^[0-9a-f]{40}$/.test(sha) && (name === ref || name === `${ref}^{}`), 'Unexpected remote tag response')
			return [name, sha]
		}))
		check((entries.get(`${ref}^{}`) ?? entries.get(ref)) === commit, 'GitCode tag points to a different commit; refusing to overwrite')
		return
	}
	await git('push built commit as GitCode tag', ['push', '--no-follow-tags', REPOSITORY, `${commit}:${ref}`])
	const pushed = await git('verify pushed GitCode tag', ['ls-remote', '--tags', REPOSITORY, ref, `${ref}^{}`])
	check(pushed === `${commit}\t${ref}`, 'GitCode tag verification failed')
}

function uploadTarget(data, token) {
	check(typeof data.url === 'string' && data.url.length <= 32_768 && !/[\s\x00-\x1f\x7f]/.test(data.url), 'Invalid upload URL')
	let url
	try { url = new URL(data.url) } catch { throw new Error('Invalid upload URL') }
	console.log(`Upload target host: ${url.hostname}; protocol=${url.protocol}; port=${url.port || 'default'}; queryPresent=${Boolean(url.search)}`)
	check(url.protocol === 'https:' && !url.username && !url.password && !url.hash &&
		(!url.port || url.port === '443') && OBS_HOST.test(url.hostname), 'Upload host is outside the Huawei OBS allowlist')
	check(!data.url.includes(token), 'Upload URL unexpectedly contains the GitCode credential')
	check(data.headers && typeof data.headers === 'object' && !Array.isArray(data.headers), 'Missing upload headers')
	console.log(`Upload header names: ${Object.keys(data.headers).slice(0, 20).map(diagnosticCode).join(', ')}`)
	const expected = new Set(['x-obs-meta-project-id', 'x-obs-acl', 'x-obs-callback', 'content-type'])
	const headers = {}
	for (const [name, value] of Object.entries(data.headers)) {
		const lower = name.toLowerCase()
		check(expected.delete(lower) && typeof value === 'string' && value.length > 0 &&
			value.length <= 32_768 && !/[\x00-\x1f\x7f]/.test(value) && !value.includes(token), 'Unexpected or unsafe upload header')
		headers[name] = value
	}
	check(expected.size === 0, 'Missing required OBS upload headers')
	return { url, headers }
}

async function uploadFile(target, file) {
	const handle = await open(file.path, constants.O_RDONLY | constants.O_NOFOLLOW)
	try {
		const stat = await handle.stat()
		check(stat.isFile() && stat.size === file.size && stat.ino === file.stat.ino &&
			stat.dev === file.stat.dev && stat.mtimeMs === file.stat.mtimeMs, 'Artifact changed after preflight')
		let bytes = 0
		const hash = createHash('sha256')
		const guard = new Transform({
			transform(chunk, encoding, callback) {
				bytes += chunk.length
				if (bytes > file.size) return callback(new Error('Artifact grew'))
				hash.update(chunk)
				callback(null, chunk)
			},
			flush(callback) {
				callback(bytes === file.size && hash.digest('hex') === file.sha256 ? null : new Error('Artifact changed'))
			},
		})
		let streamComplete
		const result = await requestJson(target.url, {
			method: 'PUT', headers: { ...target.headers, 'Content-Length': String(file.size) }, timeout: UPLOAD_TIMEOUT_MS,
			upload(request) {
				streamComplete = pipeline(handle.createReadStream({ autoClose: false }), guard, request)
				return streamComplete
			},
		})
		await streamComplete
		check(result.status >= 200 && result.status < 300, `OBS upload failed (HTTP ${result.status})`)
	} finally {
		await handle.close()
	}
}

function verifyAssets(release, files) {
	check(Array.isArray(release.assets), 'GitCode release has no assets array')
	for (const file of files) {
		const matches = release.assets.filter((asset) => asset.name === file.name)
		check(matches.length === 1, `Missing or duplicate release attachment: ${file.name}`)
		const asset = matches[0]
		check(typeof asset.browser_download_url === 'string' && asset.browser_download_url.startsWith('https://'), 'Attachment has no HTTPS download URL')
		if (asset.size !== undefined && asset.size !== null) {
			check(Number(asset.size) === file.size, `Attachment size mismatch: ${file.name}`)
		}
		if (asset.digest !== undefined && asset.digest !== null && asset.digest !== '') {
			check(typeof asset.digest === 'string', 'Invalid attachment digest')
			const match = /^(?:(sha256|sha512):)?([0-9a-f]+)$/i.exec(asset.digest)
			check(match, 'Unsupported attachment digest format')
			const algorithm = match[1]?.toLowerCase() ?? (match[2].length === 64 ? 'sha256' : 'sha512')
			check(match[2].toLowerCase() === file[algorithm], `Attachment digest mismatch: ${file.name}`)
		}
	}
}

async function summary(text) {
	if (process.env.GITHUB_STEP_SUMMARY) await appendFile(process.env.GITHUB_STEP_SUMMARY, `${text}\n`)
}

async function main() {
	check(Number(process.versions.node.split('.')[0]) >= 22, 'Node.js 22 or newer is required')
	check(process.argv.length === 3, 'Usage: node .github/scripts/publish-gitcode.mjs <artifact-directory>')
	required('GITCODE_USERNAME')
	const token = required('GITCODE_TOKEN')
	const tag = required('RELEASE_TAG')
	const version = required('RELEASE_VERSION')
	const commit = required('RELEASE_COMMIT')
	check(/^[0-9a-f]{40}$/.test(commit), 'RELEASE_COMMIT must be the full built commit SHA')
	check(/^(?:v)?[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(tag) &&
		tag.replace(/^v/, '') === version, 'Release tag and version do not match')
	const files = await preflight(resolve(process.argv[2]), version)
	console.log(`Preflight passed: ${files.length} signed installer/signature files`)
	const api = apiClient(token)
	const encodedTag = encodeURIComponent(tag)
	const releasePath = `/releases/tags/${encodedTag}`
	const link = `${WEB}/releases/tag/${encodedTag}`
	check(await api(releasePath, { allowMissing: true }) === null,
		'GitCode release already exists; manually resolve it before retrying (no overwrite or deletion)')
	await withGit((git) => pushTag(git, tag, commit))
	let stage = 'create prerelease'
	try {
		console.log(`GitCode stage: ${stage}`)
		await api('/releases', {
			method: 'POST',
			body: {
				tag_name: tag,
				name: `Celestial Launcher ${version}`,
				body: 'GitCode 预发布暂存：附件上传和核验完成后由维护者人工推广。',
				target_commitish: commit,
				release_status: 'pre',
			},
		})
		console.log('GitCode stage completed: create prerelease')
		for (const file of files) {
			stage = `obtain upload target for ${file.name}`
			console.log(`GitCode stage: ${stage}`)
			const data = await api(`/releases/${encodedTag}/upload_url?file_name=${encodeURIComponent(file.name)}`)
			stage = `validate upload target for ${file.name}`
			console.log(`GitCode stage: ${stage}`)
			const target = uploadTarget(data, token)
			stage = `upload ${file.name}`
			console.log(`GitCode stage: ${stage}; host=${target.url.hostname}; bytes=${file.size}`)
			await uploadFile(target, file)
			console.log(`GitCode stage completed: ${stage}`)
		}
		stage = 'verify registered release attachments'
		console.log(`GitCode stage: ${stage}`)
		verifyAssets(await api(releasePath), files)
		console.log(`GitCode prerelease verified: ${link}; manual promotion required`)
		await summary(`### GitCode 预发布已暂存\n\n[查看 ${tag}](${link})：${files.length} 个附件已逐一核验。状态始终为 **pre**（公开预发布，并非草稿），稳定版本也不会自动推广。请维护者确认完整性后在 GitCode 发布页人工推广；测试版本可保持预发布。`)
	} catch (error) {
		const details = gitDiagnostic(String(error.message)).slice(0, 12_000)
		console.error(`GitCode stage failed: ${stage}\n${details}`)
		await summary(`### GitCode 暂存未完成\n\n[检查 ${tag}](${link})。失败阶段：${stage}。可能已存在部分上传的公开预发布；请勿推广。脚本不会删除或覆盖附件，同名 release 会阻止重跑，必须先人工处理。`)
		throw new Error(`GitCode staging failed (${stage}):\n${details}\nNo assets were deleted or overwritten.`)
	}
}

export { apiClient, inspectFile, uploadTarget, uploadFile, verifyAssets, withGit, pushTag, summary, gitDiagnostic }

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	main().catch((error) => {
		console.error(`GitCode publisher: ${error.message}`)
		process.exitCode = 1
	})
}
