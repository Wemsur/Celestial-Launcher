// src/helpers/backgroundApply.ts
import { invoke } from '@tauri-apps/api/core'

// 全局唯一锁，防止并发调用
let isApplying = false

export async function applyBackground(forceDelay = false) {
    // 如果正在应用，等待当前任务完成或直接返回（防并发）
    if (isApplying) {
        await new Promise(resolve => {
            // 简单的方式：等待一小段时间后重试，或者直接忽略
            setTimeout(resolve, forceDelay ? 500 : 0);
        });
        return; // 或者再次检查最新状态？这里简化处理
    }

    isApplying = true;
    try {
        console.log('applyBackground called');

        // 【第一步】彻底清理所有背景痕迹
        document.body.classList.remove('custom-bg-active');

        // 移除所有可能存在的背景 img 标签
        const oldImgs = document.querySelectorAll('.custom-user-bg-img');
        oldImgs.forEach(img => img.remove());

        // 清除 CSS 变量
        document.documentElement.style.removeProperty('--app-custom-background');

        // 移除注入的 runtime style（如果存在）
        const oldStyle = document.getElementById('custom-bg-runtime-style');
        if (oldStyle) oldStyle.remove();

        // 【第二步】获取新背景数据
        const base64Data = await invoke<string>('get_background_as_base64');
        console.log('Got background data, length:', base64Data?.length);

        if (base64Data && base64Data.length > 0) {
            // 【第三步】重新设置新背景
            document.documentElement.style.setProperty('--app-custom-background', `url(${base64Data})`);
            document.body.classList.add('custom-bg-active');

            // 创建新的 img 标签（确保只存在一个）
            const bgImg = document.createElement('img');
            bgImg.src = base64Data;
            bgImg.style.position = 'fixed';
            bgImg.style.top = '0';
            bgImg.style.left = '0';
            bgImg.style.width = '100vw';
            bgImg.style.height = '100vh';
            bgImg.style.objectFit = 'cover';
            bgImg.style.zIndex = '-9999';
            bgImg.style.pointerEvents = 'none';
            bgImg.classList.add('custom-user-bg-img');

            // 确保在 body 最前面
            document.body.prepend(bgImg);

            // 【第四步】注入样式规则（幂等检查）
            let styleEl = document.getElementById('custom-bg-runtime-style');
            if (!styleEl) {
                styleEl = document.createElement('style');
                styleEl.id = 'custom-bg-runtime-style';
                styleEl.innerHTML = `
                    #app, .app-container, body {
                        background-image: var(--app-custom-background) !important;
                        background-size: cover !important;
                        background-position: center !important;
                        background-repeat: no-repeat !important;
                        background-attachment: fixed !important;
                    }
                `;
                document.head.appendChild(styleEl);
            }

            console.log('Background applied successfully')
        } else {
            console.log('No background set, cleared')
        }
    } catch (error) {
        console.error('Failed to apply background:', error);
        // 出错时不改变现有状态，保持原样
    } finally {
        isApplying = false;
    }
}

// 导出一个辅助函数用于设置加载状态（供 UI 禁用时使用）
export function setApplyingState(loading: boolean) {
    // 可以通过全局变量或 store 通知 UI 加载状态
    // 这里简单用一个标志，实际可能需要更完善的通信机制
    window['__backgroundApplying'] = loading;
}