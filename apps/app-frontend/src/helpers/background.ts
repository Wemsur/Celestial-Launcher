import {convertFileSrc, invoke} from '@tauri-apps/api/core'

/**
 * 将前端提取到的背景图片二进制数据投递给 Rust 后端进行持久化存储
 */
export async function process_dragged_background(
    backgroundBlob: Uint8Array,
    fileName: string
): Promise<string> {
    // 去掉 'plugin:xxx' 前缀，直接呼叫全局命令
    return await invoke('save_background_image', {
        backgroundBlob,
        fileName,
    })
}

export function getAssetUrl(absolutePath: string): string {
    if (!absolutePath) return ''
    // Tauri v2 将绝对路径转换为 asset:// 协议或标准安全本地 URL
    return convertFileSrc(absolutePath)
}

export async function get_background_url(): Promise<string> {
    try {
        const base64Url: string = await invoke('get_background_as_base64')
        return base64Url
    } catch (e) {
        console.warn("无法获取背景图:", e)
        return ''
    }
}