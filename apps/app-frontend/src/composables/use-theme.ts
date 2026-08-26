import { computed, reactive, ref, watch } from 'vue'

export const THEME_OPTIONS = ['customdark', 'customlight', 'oled', 'retro', 'elegant', 'antiquedark', 'system'] as const

export type ColorTheme = (typeof THEME_OPTIONS)[number]
type Theme = Exclude<ColorTheme, 'system'>

const preferred = ref<ColorTheme>('customdark')
const preview = ref<ColorTheme | null>(null)
const advancedRendering = ref(true)
const syncAcrossDevices = ref(false)

const savedHue = localStorage.getItem('celestial_hue_value')
const hueValue = ref<number>(savedHue ? Number(savedHue) : 38)

/** Themes "system" resolves to, following the OS light/dark preference. */
const SYSTEM_DARK_THEME: Theme = 'customdark'
const SYSTEM_LIGHT_THEME: Theme = 'customlight'

const nativeThemeQuery = window.matchMedia('(prefers-color-scheme: dark)')
const native = ref<Theme>(
	nativeThemeQuery.matches ? SYSTEM_DARK_THEME : SYSTEM_LIGHT_THEME,
)
const active = computed<Theme>(() => {
	const selectedTheme = preview.value ?? preferred.value
	return selectedTheme === 'system' ? native.value : selectedTheme
})

nativeThemeQuery.addEventListener('change', (event) => {
	native.value = event.matches ? SYSTEM_DARK_THEME : SYSTEM_LIGHT_THEME
})

watch(
	active,
	(theme) => {
		const html = document.documentElement
		for (const option of THEME_OPTIONS) {
			html.classList.remove(`${option}-mode`)
		}
		html.classList.add(`${theme}-mode`)
	},
	{ immediate: true },
)

const customBgBlur = ref<boolean>(() => localStorage.getItem('celestial_custom_bg_blur') === 'true')

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

function toggleBgBlur(enabled: boolean): void {
	customBgBlur.value = enabled
	localStorage.setItem('celestial_custom_bg_blur', String(enabled))
	document.body.classList.toggle('custom-bgblur', enabled)
}

const theme = reactive({
	preferred,
	preview,
	active,
	native,
	syncAcrossDevices,
	advancedRendering,
	hueValue,
	customBgBlur,
	options: THEME_OPTIONS,
	loadHueValue,
	saveHueValue,
	toggleBgBlur,
})

export function useTheme() {
	return theme
}
