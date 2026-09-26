import { getVersion } from '@tauri-apps/api/app'
import {invoke} from "@tauri-apps/api/core";
import { listen } from '@tauri-apps/api/event'

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
    /** 该源的 "latest release" 接口。三家（Gitea/Forgejo/GitHub）返回的 JSON
     *  字段兼容：tag_name、draft、prerelease、assets[].browser_download_url。 */
    latestUrl: string
}

// 优先级：自部署 wemgit → git.gay → GitHub（最次）。
const UPDATE_SOURCES: UpdateSource[] = [
    {
        id: 'wemgit',
        label: 'wemgit (download.celestial.byside.top)',
        latestUrl:
            'https://download.celestial.byside.top/api/v1/repos/celestial/Celestial-Launcher/releases/latest',
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

/** 语义化版本比较，返回 -1/0/1 */
function compareVersions(a: string, b: string): number {
    const parse = (v: string) => v.split('.').map(Number)
    const [a1, a2, a3] = parse(a)
    const [b1, b2, b3] = parse(b)
    if (a1 !== b1) return a1 - b1
    if (a2 !== b2) return a2 - b2
    return a3 - b3
}

/** 按平台选择最优安装文件：优先 .msi，其次 .exe */
function selectAsset(assets: ReleaseAsset[]): ReleaseAsset | null {
    const byExtn = (ext: string) =>
        assets.find(a => a.name.toLowerCase().endsWith(ext))
    return byExtn('.exe') ?? byExtn('.msi') ?? assets[0] ?? null
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

    // 记录每个源的结局，最后打印一份总表，方便看清"为什么选了 B 而不是 A"。
    const trail: { source: UpdateSource; outcome: string }[] = []

    console.log(
        `[updater] 开始检查更新，当前版本 ${currentVersion}，优先级顺序: ${UPDATE_SOURCES.map(
            (s) => s.id,
        ).join(' → ')}`,
    )

    for (let i = 0; i < UPDATE_SOURCES.length; i++) {
        const source = UPDATE_SOURCES[i]
        const prefix = `[updater] [${i + 1}/${UPDATE_SOURCES.length}] ${source.label}`
        console.log(`${prefix}: 请求 ${source.latestUrl}`)

        const started = performance.now()
        let body: string
        try {
            // 走 Rust（reqwest）而非 webview fetch：绕开 CORS。自部署 Gitea / CDN
            // 不回 Access-Control-Allow-Origin 时，前端 fetch 会 "Failed to fetch"，
            // 而 Rust 侧不受 CORS 约束。Rust 出错时抛字符串（"HTTP 404: ..." 等）。
            body = await invoke<string>('fetch_release_metadata', { url: source.latestUrl })
        } catch (e) {
            const msg = typeof e === 'string' ? e : ((e as Error)?.message ?? String(e))
            const ms = Math.round(performance.now() - started)
            const reason = msg.startsWith('HTTP ')
                ? `${msg}（耗时 ${ms}ms）`
                : `请求失败（${msg}），耗时 ${ms}ms。常见原因：主机不可达 / DNS 失败 / 超时 / TLS 失败`
            console.warn(`${prefix}: ✗ 跳过 — ${reason}`)
            trail.push({ source, outcome: `跳过（${reason}）` })
            continue
        }
        const ms = Math.round(performance.now() - started)

        let release: Release
        try {
            release = JSON.parse(body)
        } catch (e) {
            const err = e as Error
            const reason = `响应不是合法 JSON（${err.message}），可能是错误页而非 API 响应`
            console.warn(`${prefix}: ✗ 跳过 — ${reason}`)
            trail.push({ source, outcome: `跳过（${reason}）` })
            continue
        }
        console.log(`${prefix}: 成功（耗时 ${ms}ms），tag=${release.tag_name}`)

        // 跳过 draft / prerelease
        if (release.draft || release.prerelease) {
            const reason = `最新发布是 ${release.draft ? 'draft' : 'prerelease'}，不作为正式更新`
            console.log(`${prefix}: ✗ 跳过 — ${reason}`)
            trail.push({ source, outcome: `跳过（${reason}）` })
            continue
        }

        const latestVersion = release.tag_name.replace(/^v/, '')
        const cmp = compareVersions(latestVersion, currentVersion)
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

        const asset = selectAsset(release.assets)
        if (!asset) {
            const reason = `版本 ${latestVersion} 有更新，但该源没有匹配的安装包（assets: ${
                release.assets?.map((a) => a.name).join(', ') || '空'
            }）`
            console.warn(`${prefix}: ✗ 跳过 — ${reason}`)
            trail.push({ source, outcome: `跳过（${reason}）` })
            continue
        }

        console.log(
            `${prefix}: ✓ 采用此源 — 版本 ${latestVersion} > 当前 ${currentVersion}，安装包 ${asset.name}（${asset.browser_download_url}）`,
        )
        trail.push({ source, outcome: `✓ 采用（版本 ${latestVersion}，安装包 ${asset.name}）` })
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
    appUpdateState.progress.value = 0

    // The Rust side streams the download and emits `app-update-progress` with a
    // 0..1 fraction; mirror it into the shared state so the settings-page
    // progress bar fills up the way it did with the original Modrinth updater.
    const unlisten = await listen<number>('app-update-progress', (event) => {
        appUpdateState.progress.value = event.payload
    })

    try {
        const filename = assetUrl.split('/').pop() ?? 'update.exe'
        localStorage.setItem('celestial-last-msi-filename', filename)
        const result = await invoke('download_and_run_msi', { assetUrl })
        localStorage.setItem('celestial-last-msi-filename', result as string)

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
        appUpdateState.progress.value = 0
        console.error('Update download failed:', e)
    } finally {
        unlisten()
    }
}
