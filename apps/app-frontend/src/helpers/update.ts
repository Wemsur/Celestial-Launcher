import { getVersion } from '@tauri-apps/api/app'
import {invoke} from "@tauri-apps/api/core";

import {
    appUpdateState,
    markAppUpdateActionable,
    markAppUpdatePopupShown,
} from '@/providers/app-update'

const REPO_OWNER = 'celestial-launcher'
const REPO_NAME = 'Celestial'

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
 * 返回 null 表示已是最新或检查失败。
 */
export async function checkForUpdate(): Promise<{
    currentVersion: string
    latestVersion: string
    release: Release
    asset: ReleaseAsset
} | null> {
    try {
        const resp = await fetch(
            `https://git.gay/api/v1/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest`
        )
        if (!resp.ok) return null
        const release: Release = await resp.json()

        // 跳过 draft / prerelease
        if (release.draft || release.prerelease) return null

        const currentVersion = await getVersion()
        const latestVersion = release.tag_name.replace(/^v/, '')
        if (compareVersions(latestVersion, currentVersion) <= 0) return null

        const asset = selectAsset(release.assets)
        if (!asset) return null

        return { currentVersion, latestVersion, release, asset }
    } catch (e) {
        console.error('Update check failed:', e)
        return null
    }
}

/**
 * 下载 release asset 并写入缓存目录，然后启动安装器。
 */
export async function downloadAndRunRelease(
    assetUrl: string,
    version: string
): Promise<void> {
    appUpdateState.downloading.value = true
    appUpdateState.progress.value = 0

    try {
        const filename = assetUrl.split('/').pop() ?? 'update.exe'
        localStorage.setItem('celestial-last-msi-filename', filename)
        const result = await invoke('download_and_run_msi', { assetUrl })
        localStorage.setItem('celestial-last-msi-filename', result as string)

        appUpdateState.downloading.value = false
        appUpdateState.finishedDownloading.value = true
        markAppUpdateActionable(version, 'downloaded')
        markAppUpdatePopupShown(version, 'downloaded')
    } catch (e) {
        appUpdateState.downloading.value = false
        appUpdateState.progress.value = 0
        console.error('Update download failed:', e)
    }
}
