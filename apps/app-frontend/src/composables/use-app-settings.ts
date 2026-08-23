import { reactive, ref, watch } from 'vue'

export const DEFAULT_FEATURE_FLAGS = {
	project_background: false,
	page_path: false,
	worlds_in_home: true,
	server_project_qa: false,
	show_version_environment_column: false,
	server_ram_as_bytes_always_on: false,
	always_show_app_controls: false,
	skip_non_essential_warnings: false,
	skip_unknown_pack_warning: false,
	pride_fundraiser: true,
	i18n_debug: false,
	show_instance_play_time: true,
	compact_instance_cards: false,
	advanced_filters_collapsed: true,
	always_show_copy_details: false,
	hide_installed_modpacks: false,
	friends_active_collapsed: false,
	friends_online_collapsed: false,
	friends_offline_collapsed: true,
	friends_pending_collapsed: true,
	dismissed_photosensitivity_filter_warning: false,
}

export type FeatureFlag = keyof typeof DEFAULT_FEATURE_FLAGS
type FeatureFlags = Record<FeatureFlag, boolean>

const featureFlags = reactive<FeatureFlags>({ ...DEFAULT_FEATURE_FLAGS })


function getFeatureFlag(key: FeatureFlag): boolean {
	return featureFlags[key] ?? DEFAULT_FEATURE_FLAGS[key]
}

// Subscription system — mimics Pinia store $subscribe
type SubCallback = () => void
const subscribers = new Set<SubCallback>()

function notifySubscribers() {
	subscribers.forEach(fn => fn())
}

const appSettings = reactive({

	hideNametagSkinsPage: false,
	toggleSidebar: false,
	devMode: false,
	featureFlags,

	getFeatureFlag,
	$subscribe(callback: SubCallback) {
		subscribers.add(callback)
		return () => subscribers.delete(callback)
	},
})

// Watch any change on appSettings and notify subscribers
watch(
	() => ({
		toggleSidebar: appSettings.toggleSidebar,
		hideNametagSkinsPage: appSettings.hideNametagSkinsPage,
		devMode: appSettings.devMode,
	}),
	() => notifySubscribers(),
	{ deep: true },
)

export function useAppSettings() {
	return appSettings
}
