import { BaseDirectory,readTextFile, writeTextFile } from '@tauri-apps/plugin-fs'
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'

const CONFIG_FILE_NAME = 'custom_bg_settings.txt'

let systemThemeMq: MediaQueryList | null = null

export const DEFAULT_FEATURE_FLAGS = {
	project_background: true,//false
	page_path: false,//false
	worlds_tab: true,//false
	worlds_in_home: true,
	server_project_qa: false,
	server_ram_as_bytes_always_on: false,
	always_show_app_controls: false,
	skip_non_essential_warnings: false,
	skip_unknown_pack_warning: false,
	pride_fundraiser: true,
	i18n_debug: false,//false
	show_instance_play_time: true,
}

export const THEME_OPTIONS = ['dark', 'light', 'oled', 'elegant', 'antiquedark', 'system'] as const

export type FeatureFlag = keyof typeof DEFAULT_FEATURE_FLAGS
export type FeatureFlags = Record<FeatureFlag, boolean>
export type ColorTheme = (typeof THEME_OPTIONS)[number]

export type ThemeStore = {
	selectedTheme: ColorTheme
	advancedRendering: boolean
	customBgBlur: boolean
	hideNametagSkinsPage: boolean
	toggleSidebar: boolean

	devMode: boolean
	featureFlags: FeatureFlags
}

export const DEFAULT_THEME_STORE: ThemeStore = {
	selectedTheme: 'dark',
	customBgBlur: true,
	advancedRendering: true,
	hideNametagSkinsPage: false,
	toggleSidebar: false,

	devMode: false,
	featureFlags: DEFAULT_FEATURE_FLAGS,
}

export const useTheming = defineStore('themeStore', {
	state: () => DEFAULT_THEME_STORE,
	actions: {
		// 1. 刷新或启动时，直接找 Rust 问文件内容
		async loadCustomSettings() {
			try {
				// 调用 Rust 侧的加载命令
				this.customBgBlur = await invoke<boolean>('load_bg_blur_status')
			} catch (e) {
				console.error('[Frontend] 从 Rust 加载模糊配置失败，降级为默认值', e)
				this.customBgBlur = true
			}
			this.setBgBlurClass()
		},

		// 2. 切换开关时，直接把布尔值丢给 Rust 让它去写 AppData
		async toggleBgBlur(isActive: boolean) {
			this.customBgBlur = isActive
			this.setBgBlurClass()

			try {
				// 调用 Rust 侧的保存命令
				await invoke('save_bg_blur_status', { isActive: isActive });
			} catch (e) {
				console.error('[Frontend] 无法通过 Rust 保存模糊配置:', e)
			}
		},
		setThemeState(newTheme: ColorTheme) {
			if (THEME_OPTIONS.includes(newTheme)) {
				this.selectedTheme = newTheme
			} else {
				console.warn('Selected theme is not present. Check themeOptions.')
			}

			this.setThemeClass()
		},
		setBgBlurClass() {
			if (this.customBgBlur) {
				document.body.classList.add('custom-bgblur')
			} else {
				document.body.classList.remove('custom-bgblur')
			}
		},
		setThemeClass() {
			const html = document.getElementsByTagName('html')[0]
			for (const theme of THEME_OPTIONS) {
				html.classList.remove(`${theme}-mode`)
			}

			systemThemeMq?.removeEventListener('change', this.setThemeClass)
			systemThemeMq = null

			let theme = this.selectedTheme
			if (this.selectedTheme === 'system') {
				systemThemeMq = window.matchMedia('(prefers-color-scheme: dark)')
				systemThemeMq.addEventListener('change', this.setThemeClass)
				theme = systemThemeMq.matches ? 'dark' : 'light'
			}

			html.classList.add(`${theme}-mode`)
			this.setBgBlurClass()
		},
		getFeatureFlag(key: FeatureFlag) {
			return this.featureFlags[key] ?? DEFAULT_FEATURE_FLAGS[key]
		},
		getThemeOptions() {
			return THEME_OPTIONS
		},
	},
})
