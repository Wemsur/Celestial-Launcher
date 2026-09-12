import { prepareThemeColorTransition } from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'
import { computed, reactive, ref, watch } from 'vue'

export const THEME_OPTIONS = ['customdark', 'customlight', 'oled', 'retro', 'elegant', 'antiquedark', 'system'] as const
export const DARK_THEMES = ['customdark', 'oled', 'retro', 'elegant', 'antiquedark'] as const

export type ColorTheme = (typeof THEME_OPTIONS)[number]
export type DarkTheme = (typeof DARK_THEMES)[number]
type Theme = Exclude<ColorTheme, 'system'>
type NativeTheme = 'light' | 'dark'

const PREFERRED_THEME_KEY = 'modrinth-theme'
const PREFERRED_DARK_THEME_KEY = 'modrinth-preferred-dark-theme'

export function isDarkTheme(theme: string): theme is DarkTheme {
	return (DARK_THEMES as readonly string[]).includes(theme)
}

function loadPreferredTheme(): ColorTheme {
	try {
		const stored = window.localStorage.getItem(PREFERRED_THEME_KEY)
		if (stored && (THEME_OPTIONS as readonly string[]).includes(stored)) {
			return stored as ColorTheme
		}
	} catch {
		// storage blocked or full
	}

	for (const option of THEME_OPTIONS) {
		if (option !== 'system' && document.documentElement.classList.contains(`${option}-mode`)) {
			return option
		}
	}

	return 'customdark'
}

function loadPreferredDarkTheme(): DarkTheme {
	try {
		const stored = window.localStorage.getItem(PREFERRED_DARK_THEME_KEY)
		if (stored && isDarkTheme(stored)) {
			return stored
		}
	} catch {
		// storage blocked or full
	}

	return 'customdark'
}

const preferred = ref<ColorTheme>(loadPreferredTheme())
const preview = ref<ColorTheme | null>(null)
const preferredDark = ref<DarkTheme>(loadPreferredDarkTheme())
const advancedRendering = ref(true)
const syncAcrossDevices = ref(false)

const savedHue = localStorage.getItem('celestial_hue_value')
const hueValue = ref<number>(savedHue ? Number(savedHue) : 38)

/** Themes "system" resolves to, following the OS light/dark preference. */
const SYSTEM_LIGHT_THEME: Theme = 'customlight'

const nativeThemeQuery = window.matchMedia('(prefers-color-scheme: dark)')
const native = ref<NativeTheme>(nativeThemeQuery.matches ? 'dark' : 'light')
const active = computed<Theme>(() => {
	const selectedTheme = preview.value ?? preferred.value
	if (selectedTheme !== 'system') {
		return selectedTheme
	}

	return native.value === 'light' ? SYSTEM_LIGHT_THEME : preferredDark.value
})

nativeThemeQuery.addEventListener('change', (event) => {
	native.value = event.matches ? 'dark' : 'light'
})

watch([preferred, preview], ([selectedPreferred, selectedPreview]) => {
	const selectedTheme = selectedPreview ?? selectedPreferred
	if (isDarkTheme(selectedTheme)) {
		preferredDark.value = selectedTheme
	}
})

watch(
	preferred,
	(theme) => {
		try {
			window.localStorage.setItem(PREFERRED_THEME_KEY, theme)
		} catch {
			// storage blocked or full
		}
	},
	{ immediate: true },
)

watch(preferredDark, (theme) => {
	try {
		window.localStorage.setItem(PREFERRED_DARK_THEME_KEY, theme)
	} catch {
		// storage blocked or full
	}
})

watch(
	active,
	(theme, previousTheme) => {
		if (previousTheme && previousTheme !== theme) {
			prepareThemeColorTransition()
		}

		const html = document.documentElement
		for (const option of THEME_OPTIONS) {
			html.classList.remove(`${option}-mode`)
		}
		html.classList.add(`${theme}-mode`)
	},
	{ immediate: true },
)

/**
 * Background blur lives in `custom_backgrounds/celestial_settings.json`
 * (`blur_enabled`), read and written through the Tauri commands below.
 * localStorage is only a cache so the very first frame paints the right state
 * without waiting on the async round-trip; the file is the source of truth.
 */
const BG_BLUR_STORAGE_KEY = 'celestial_custom_bg_blur'
const BG_BLUR_DEFAULT = true

function cachedBgBlur(): boolean {
	const saved = localStorage.getItem(BG_BLUR_STORAGE_KEY)
	return saved === null ? BG_BLUR_DEFAULT : saved === 'true'
}

const customBgBlur = ref<boolean>(cachedBgBlur())

function applyBgBlur(enabled: boolean): void {
	customBgBlur.value = enabled
	localStorage.setItem(BG_BLUR_STORAGE_KEY, String(enabled))
	document.body.classList.toggle('custom-bgblur', enabled)
}

applyBgBlur(customBgBlur.value)

async function loadBgBlur(): Promise<void> {
	try {
		applyBgBlur(await invoke<boolean>('load_bg_blur_status'))
	} catch (error) {
		console.error('Failed to load background blur setting:', error)
	}
}

async function loadHueValue(): Promise<void> {
	const saved = localStorage.getItem('celestial_hue_value')
	hueValue.value = saved ? Number(saved) : 38
	document.documentElement.style.setProperty('--brand-hue', String(hueValue.value))
}

function saveHueValue(val: number): void {
	hueValue.value = val
	localStorage.setItem('celestial_hue_value', String(val))
	document.documentElement.style.setProperty('--brand-hue', String(val))
}

async function toggleBgBlur(enabled: boolean): Promise<void> {
	applyBgBlur(enabled)
	try {
		await invoke('save_bg_blur_status', { isActive: enabled })
	} catch (error) {
		console.error('Failed to save background blur setting:', error)
	}
}

/**
 * Instance cards using their own icon as a blurred backdrop. Same storage as the
 * background blur above — `instance_card_icon_bg` in
 * `custom_backgrounds/celestial_settings.json`, with localStorage as a
 * first-paint cache so cards do not visibly change style a moment after load.
 *
 * On by default — this is how instance cards are meant to look; the toggle exists
 * for people who want the flat ones back.
 */
const INSTANCE_CARD_ICON_BG_STORAGE_KEY = 'celestial_instance_card_icon_bg'
const INSTANCE_CARD_ICON_BG_DEFAULT = true

function cachedInstanceCardIconBg(): boolean {
	const saved = localStorage.getItem(INSTANCE_CARD_ICON_BG_STORAGE_KEY)
	return saved === null ? INSTANCE_CARD_ICON_BG_DEFAULT : saved === 'true'
}

const instanceCardIconBg = ref<boolean>(cachedInstanceCardIconBg())

function applyInstanceCardIconBg(enabled: boolean): void {
	instanceCardIconBg.value = enabled
	localStorage.setItem(INSTANCE_CARD_ICON_BG_STORAGE_KEY, String(enabled))
}

async function loadInstanceCardIconBg(): Promise<void> {
	try {
		applyInstanceCardIconBg(await invoke<boolean>('load_instance_card_icon_bg'))
	} catch (error) {
		console.error('Failed to load instance card icon background setting:', error)
	}
}

async function toggleInstanceCardIconBg(enabled: boolean): Promise<void> {
	applyInstanceCardIconBg(enabled)
	try {
		await invoke('save_instance_card_icon_bg', { isActive: enabled })
	} catch (error) {
		console.error('Failed to save instance card icon background setting:', error)
	}
}

function applyAccountAppearance(appearance: { auto: boolean; theme: string }): void {
	if (isDarkTheme(appearance.theme)) {
		preferredDark.value = appearance.theme
	}

	if (appearance.auto) {
		preferred.value = 'system'
		return
	}

	if ((THEME_OPTIONS as readonly string[]).includes(appearance.theme)) {
		preferred.value = appearance.theme as ColorTheme
	}
}

const theme = reactive({
	preferred,
	preview,
	preferredDark,
	active,
	native,
	syncAcrossDevices,
	advancedRendering,
	hueValue,
	customBgBlur,
	instanceCardIconBg,
	options: THEME_OPTIONS,
	loadHueValue,
	saveHueValue,
	loadBgBlur,
	toggleBgBlur,
	loadInstanceCardIconBg,
	toggleInstanceCardIconBg,
	applyAccountAppearance,
})

export function useTheme() {
	return theme
}
