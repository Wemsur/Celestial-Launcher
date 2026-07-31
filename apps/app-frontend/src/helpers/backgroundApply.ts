// src/helpers/backgroundApply.ts
import { invoke } from '@tauri-apps/api/core'

export async function applyBackground() {
    try {
        const base64Data = await invoke<string>('get_background_as_base64')

        if (base64Data && base64Data.length > 0) {
            // 【关键】直接移除旧的，不拼接前缀
            document.body.classList.remove('custom-bg-active')
            const oldImg = document.querySelector('.custom-user-bg-img')
            if (oldImg) oldImg.remove()

            // 【关键】直接使用返回值，它已经是完整 data URL
            document.documentElement.style.setProperty('--app-custom-background', `url(${base64Data})`)
            document.body.classList.add('custom-bg-active')

            const bgImg = document.createElement('img')
            bgImg.src = base64Data  // 【关键】直接赋值
            bgImg.style.position = 'fixed'
            bgImg.style.top = '0'
            bgImg.style.left = '0'
            bgImg.style.width = '100vw'
            bgImg.style.height = '100vh'
            bgImg.style.objectFit = 'cover'
            bgImg.style.zIndex = '-9999'
            bgImg.style.pointerEvents = 'none'
            bgImg.classList.add('custom-user-bg-img')
            document.body.prepend(bgImg)

            console.log('Background applied via applyBackground()')
        } else {
            document.body.classList.remove('custom-bg-active')
            const oldImg = document.querySelector('.custom-user-bg-img')
            if (oldImg) oldImg.remove()
            document.documentElement.style.removeProperty('--app-custom-background')
        }
    } catch (error) {
        console.error('Failed to apply background:', error)
    }
}