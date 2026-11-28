import { getVersion } from '@tauri-apps/api/app'
import {invoke} from "@tauri-apps/api/core";
import { listen } from '@tauri-apps/api/event'
import { arch as osArch, type as osType } from '@tauri-apps/plugin-os'

import { compareBetaTags, isBetaTag } from '@/helpers/beta-release'
import { toError } from '@/helpers/errors'
import { get as getSettings } from '@/helpers/settings'
import {
    appUpdateState,
    markAppUpdateActionable,
} from '@/providers/app-update'

interface ReleaseAsset {
    name: string
    browser_download_url: string
    size: number
}

interface Release {
    tag_name: string
    body: string
    prerelease: boolean
    draft: boolean
    assets: ReleaseAsset[]
}

/** 更新源；按数组顺序依次尝试，靠前的优先。 */
interface UpdateSource {
    id: string
    /** 打印到控制台的可读名称。 */
    label: string
    /** 该源的 latest release 接口，GitCode 响应在读取后归一化。 */
    latestUrl: string
}

type TestReleaseMode = 'gitcode' | 'github' | null

function testReleaseMode(flags: { gitcode_test_release_updates?: boolean; github_test_release_updates?: boolean }): TestReleaseMode {
	if (flags.gitcode_test_release_updates === true) return 'gitcode'
	if (flags.github_test_release_updates === true) return 'github'
	return null
}

// 优先级：GitCode → git.gay → GitHub。
const UPDATE_SOURCES: UpdateSource[] = [
	{
		id: 'gitcode',
		label: 'GitCode (Wemsur/Celestial-Launcher)',
		latestUrl: 'https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher/releases/latest?type=latest',
	},
    {
        id: 'gitgay',
        label: 'git.gay',
        latestUrl: 'https://git.gay/api/v1/repos/celestial-launcher/Celestial/releases/latest',
    },
    {
        id: 'github',
        label: 'GitHub (Wemsur/Celestial-Launcher)',
        latestUrl: 'https://api.github.com/repos/Wemsur/Celestial-Launcher/releases/latest',
    },
]

function normalizeRelease(raw: unknown, source: UpdateSource): Release {
	if (!raw || typeof raw !== 'object') throw new Error('Release response must be an object')
	const value = raw as Record<string, unknown>
	if (value.error_code !== undefined || typeof value.tag_name !== 'string' || !value.tag_name.trim() || !Array.isArray(value.assets)) {
		throw new Error('Release response has no valid tag/assets or contains an API error')
	}
	const gitcode = source.id === 'gitcode' || source.id === 'gitcode-preview-test'
	if (gitcode && !['pre', 'latest'].includes(String(value.release_status))) throw new Error('Unknown GitCode release status')
	if (typeof value.prerelease !== 'boolean' || (!gitcode && typeof value.draft !== 'boolean')) throw new Error('Release visibility is not explicit')
	const tag = value.tag_name
	const assets = value.assets.map((entry: unknown): ReleaseAsset => {
		if (!entry || typeof entry !== 'object') throw new Error('Invalid release asset')
		const asset = entry as Record<string, unknown>
		if (typeof asset.name !== 'string' || !asset.name || /[\\/\x00-\x1f\x7f]/.test(asset.name)) throw new Error('Invalid release asset name')
		if (!gitcode && typeof asset.browser_download_url !== 'string') throw new Error('Release asset has no download URL')
		return {
			name: asset.name,
			browser_download_url: gitcode
				? `https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher/releases/${encodeURIComponent(tag)}/attach_files/${encodeURIComponent(asset.name)}/download`
				: asset.browser_download_url as string,
			size: typeof asset.size === 'number' && Number.isSafeInteger(asset.size) && asset.size >= 0 ? asset.size : 0,
		}
	})
	return {
		tag_name: tag, body: typeof value.body === 'string' ? value.body : '',
		draft: value.draft === true,
		prerelease: value.prerelease || (gitcode && value.release_status !== 'latest') || tag.replace(/^v/, '').split('+')[0].includes('-'),
		assets,
	}
}

async function latestBetaRelease(source: UpdateSource): Promise<Release> {
	const pageSize = 100
	const maxPages = 20
	let latest: Release | null = null
	for (let page = 1; page <= maxPages; page++) {
		const url = new URL(source.latestUrl)
		url.searchParams.set('page', String(page))
		url.searchParams.set('per_page', String(pageSize))
		console.info(`[updater] ${source.label}: 获取发布列表第 ${page} 页`)
		const raw: unknown = JSON.parse(await invoke<string>('fetch_release_metadata', { url: url.href }))
		if (!Array.isArray(raw)) throw new Error('Release list must be an array')
		if (raw.length > pageSize) throw new Error('Release list exceeds the requested page size')
		for (const entry of raw) {
			if (!entry || typeof entry !== 'object') continue
			const item = entry as Record<string, unknown>
			if (item.draft === true || typeof item.tag_name !== 'string' || !isBetaTag(item.tag_name)) continue
			const release = normalizeRelease(entry, source)
			if (!latest || (compareBetaTags(release.tag_name, latest.tag_name) ?? 0) > 0) latest = release
		}
		if (raw.length < pageSize) {
			if (!latest) throw new Error('没有找到已发布的 beta Release')
			console.info(`[updater] ${source.label}: 选择最高 beta 版本 ${latest.tag_name}`)
			return latest
		}
	}
	throw new Error('发布列表超过分页上限，无法确认最新 beta，停止本次测试更新')
}

/** 语义化版本比较，返回 -1/0/1 */
function compareVersions(a: string, b: string): number {
    const parse = (v: string) => v.split('.').map(Number)
    const [a1, a2, a3] = parse(a)
    const [b1, b2, b3] = parse(b)
    if (a1 !== b1) return a1 - b1
    if (a2 !== b2) return a2 - b2
    return a3 - b3
}

/** 当前设备平台，token 与发布产物命名规范对齐（os=windows/macos/linux，
 *  arch=x64/arm64/universal）。发布产物命名：
 *  CelestialLauncher-<version>-<os>-<arch>[-setup].<ext> */
interface PlatformInfo {
    os: string
    /** 归一化架构 token：x86_64→x64、aarch64→arm64；其余原样。 */
    arch: string
    label: string
}

function currentPlatform(): PlatformInfo {
    const os = osType() // 'windows' | 'macos' | 'linux' | ...
    const rawArch = osArch() // 'x86_64' | 'aarch64' | ...
    const arch = rawArch === 'x86_64' ? 'x64' : rawArch === 'aarch64' ? 'arm64' : rawArch
    return { os, arch, label: `${os}-${arch}` }
}

// 每个 OS 的安装包偏好顺序（小写扩展名，靠前优先）。
const PREFERRED_EXT: Record<string, string[]> = {
    windows: ['.exe', '.msi'], // NSIS -setup.exe 优先，其次 msi
    macos: ['.dmg', '.app.tar.gz'], // dmg 手动安装优先，tar 是更新包
    linux: ['.appimage', '.deb', '.rpm'],
}

/**
 * 按当前平台从 release 资产里挑安装包。
 * 依据规范化文件名里的 `-<os>-` 和 `-<arch>` 段做匹配：macOS 是 universal 包，
 * 只匹配 os；其它平台 os + arch 都要命中。命不中返回 null（宁可不更新也不装错架构）。
 */
function selectAsset(assets: ReleaseAsset[], platform: PlatformInfo): ReleaseAsset | null {
    const exts = PREFERRED_EXT[platform.os] ?? []
    const matchesPlatform = (name: string) => {
        const n = name.toLowerCase()
        if (!n.includes(`-${platform.os}-`)) return false
        if (platform.os === 'macos') return true // universal 包，不区分架构
        return n.includes(`-${platform.arch}`)
    }
    for (const ext of exts) {
        const found = assets.find(
            (a) => matchesPlatform(a.name) && a.name.toLowerCase().endsWith(ext),
        )
        if (found) return found
    }
    return null
}

/**
 * 检查是否有新版本可用。
 * 按优先级依次尝试各更新源，返回第一个成功的结果。
 * 返回 null 表示所有源都已是最新或检查失败。
 */
export async function checkForUpdate(): Promise<{
    currentVersion: string
    latestVersion: string
    release: Release
    asset: ReleaseAsset
    source: UpdateSource
} | null> {
	const currentVersion = await getVersion()
	const platform = currentPlatform()
	const mode = testReleaseMode((await getSettings()).feature_flags)
	const testingPrerelease = mode !== null
	const sources: UpdateSource[] = mode === 'gitcode'
		? [{
				id: 'gitcode-preview-test',
				label: 'GitCode 最新 beta 测试发布',
				latestUrl: 'https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher/releases',
			}]
		: mode === 'github'
			? [{
					id: 'github-preview-test',
					label: 'GitHub 最新 beta 测试发布',
					latestUrl: 'https://api.github.com/repos/Wemsur/Celestial-Launcher/releases',
				}]
			: UPDATE_SOURCES

	const trail: { source: UpdateSource; outcome: string }[] = []

	console.log(
		`[updater] 开始检查更新，当前版本 ${currentVersion}，平台 ${platform.label}，优先级顺序: ${sources.map(
			(s) => s.id,
		).join(' → ')}`,
	)

	for (let i = 0; i < sources.length; i++) {
		const source = sources[i]
		const prefix = `[updater] [${i + 1}/${sources.length}] ${source.label}`
        console.log(`${prefix}: 请求 ${source.latestUrl}`)

        const started = performance.now()
        let release: Release
        try {
			release = testingPrerelease
				? await latestBetaRelease(source)
				: normalizeRelease(JSON.parse(await invoke<string>('fetch_release_metadata', { url: source.latestUrl })), source)
        } catch (e) {
            const msg = typeof e === 'string' ? e : ((e as Error)?.message ?? String(e))
            const ms = Math.round(performance.now() - started)
            const reason = `请求或发布数据校验失败（${msg}），耗时 ${ms}ms`
            console.warn(`${prefix}: ✗ 跳过 — ${reason}`)
            trail.push({ source, outcome: `跳过（${reason}）` })
            continue
        }
        const ms = Math.round(performance.now() - started)
        console.log(`${prefix}: 成功（耗时 ${ms}ms），tag=${release.tag_name}`)

		if (release.draft || (release.prerelease && !testingPrerelease)) {
			const reason = `最新发布是 ${release.draft ? 'draft' : 'prerelease'}，不作为更新`
			console.log(`${prefix}: ✗ 跳过 — ${reason}`)
			trail.push({ source, outcome: `跳过（${reason}）` })
			continue
		}

		const latestVersion = release.tag_name.replace(/^v/, '')
		// 开发测试允许重复验证同一个包，不依赖正式版的版本比较。
		const cmp = testingPrerelease ? 1 : compareVersions(latestVersion, currentVersion)
        if (cmp <= 0) {
            const reason =
                cmp === 0
                    ? `版本 ${latestVersion} 与当前相同，已是最新`
                    : `版本 ${latestVersion} 低于当前 ${currentVersion}`
            console.log(`${prefix}: ○ 命中优先源但无需更新 — ${reason}`)
            trail.push({ source, outcome: `已是最新（${reason}）` })
            // 命中的是当前可达的最高优先源，信任它：不再往下问低优先级源。
            console.log(`[updater] 采用优先源 ${source.label} 的结论：无可用更新`)
            printTrail(trail)
            return null
        }

        const asset = selectAsset(release.assets, platform)
		if (asset && (testingPrerelease || source.id === 'gitcode') && !release.assets.some((entry) => entry.name === `${asset.name}.sig`)) {
			const reason = `${source.label} 安装包 ${asset.name} 缺少配对签名，停止采用该发布`
			console.warn(`${prefix}: ✗ 跳过 — ${reason}`)
			trail.push({ source, outcome: reason })
			continue
		}
        if (!asset) {
            const reason = `版本 ${latestVersion} 有更新，但没有匹配当前平台 ${platform.label} 的安装包（assets: ${
                release.assets?.map((a) => a.name).join(', ') || '空'
            }）`
            console.warn(`${prefix}: ✗ 跳过 — ${reason}`)
            trail.push({ source, outcome: `跳过（${reason}）` })
            continue
        }

		if (testReleaseMode((await getSettings()).feature_flags) !== mode) {
			console.info('[updater] 更新模式已改变，忽略本次检查结果')
			return null
		}

		const outcome = testingPrerelease
			? `测试发布 ${latestVersion}，安装包 ${asset.name}（不比较版本高低）`
			: `版本 ${latestVersion} > 当前 ${currentVersion}，安装包 ${asset.name}`
		console.log(`${prefix}: ✓ 采用此源 — ${outcome}（${asset.browser_download_url}）`)
		trail.push({ source, outcome: `✓ 采用（${outcome}）` })
		printTrail(trail)
		return { currentVersion, latestVersion, release, asset, source }
    }

    console.error('[updater] 所有更新源均检查失败或无更新')
    printTrail(trail)
    return null
}

/** 打印各源结局总表，一眼看清为什么选了/没选某个源。 */
function printTrail(trail: { source: UpdateSource; outcome: string }[]): void {
    console.log('[updater] 各源结局汇总（按优先级）:')
    trail.forEach((t, i) => {
        console.log(`[updater]   ${i + 1}. ${t.source.label} — ${t.outcome}`)
    })
}

/**
 * 下载 release asset 并写入缓存目录，然后启动安装器。
 */
export async function downloadAndRunRelease(
    assetUrl: string,
    version: string,
    sourceLabel?: string,
): Promise<void> {
    console.log(
        `[updater] 开始下载更新${sourceLabel ? `（源: ${sourceLabel}）` : ''}: ${assetUrl}`,
    )
	appUpdateState.downloading.value = true
	appUpdateState.finishedDownloading.value = false
	appUpdateState.progress.value = 0

    // The Rust side streams the download and emits `app-update-progress` with a
    // 0..1 fraction; mirror it into the shared state so the settings-page
    // progress bar fills up the way it did with the original Modrinth updater.
    const unlisten = await listen<number>('app-update-progress', (event) => {
        appUpdateState.progress.value = event.payload
    })

    try {
        // The download is verified in Rust before it is written, and the file it
        // lands in never comes back here: a path the page can name is a path
        // anything running in the page can name, and it gets executed.
		await invoke('download_and_run_msi', { assetUrl })
		console.info('[updater] 更新包下载完成，签名验证通过')

        appUpdateState.progress.value = 1
        appUpdateState.downloading.value = false
        appUpdateState.finishedDownloading.value = true
        // Only record that the "downloaded" stage is now actionable. Marking the
        // popup as already shown here is what suppressed the restart prompt:
        // getNextAppUpdatePopupTime() returns null once popupShownAt is set, so
        // showDelayedUpdatePopup() bailed before ever adding the toast. App.vue
        // marks it shown after it actually renders the notification.
        markAppUpdateActionable(version, 'downloaded')
	} catch (e) {
		appUpdateState.downloading.value = false
		appUpdateState.finishedDownloading.value = false
		appUpdateState.progress.value = 0
		console.error('Update download failed:', e)
		throw toError(e)
	} finally {
		unlisten()
	}
}
