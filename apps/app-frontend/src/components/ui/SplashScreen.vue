<template>
	<Transition name="splash-fade" @after-leave="onAfterLeave">
		<div v-if="!doneLoading" class="splash-screen dark">
			<div class="app-logo-wrapper" data-tauri-drag-region>
                <div class="h-10">
                    <svg
                        class="app-logo"
                        viewBox="0 0 1215 1675"
                         xml:space="preserve"
                         style="
                            fill-rule: evenodd;
                            clip-rule: evenodd;
                            stroke-linejoin: round;
                            stroke-miterlimit: 2;
                            margin-bottom: -133px;
                            margin-top:-100px
                        ">
                        <path d="M529.495 101C663.281 101 782.129 164.953 857.137 263.956L856.966 264.085C864.112 271.948 868.468 282.393 868.468 293.855C868.468 318.31 848.644 338.134 824.19 338.134C808.367 338.134 794.486 329.833 786.656 317.351L786.546 317.435C727.71 239.745 634.464 189.556 529.495 189.556C351.524 189.556 207.249 333.83 207.249 511.802C207.249 689.773 351.524 834.048 529.495 834.048C634.464 834.048 727.71 783.858 786.546 706.168L786.656 706.251C794.487 693.77 808.368 685.471 824.19 685.471C848.644 685.471 868.468 705.294 868.468 729.748C868.468 741.21 864.112 751.655 856.966 759.518L857.137 759.646C782.129 858.65 663.281 922.603 529.495 922.604C302.616 922.604 118.694 738.681 118.694 511.802C118.694 284.922 302.616 101 529.495 101Z" fill="#E5C981"/>
                        <ellipse cx="530.503" cy="511.58" rx="198.812" ry="198.685" fill="#E5C981"/>
                        <ellipse cx="530.503" cy="511.58" rx="198.812" ry="198.685" fill="url(#paint0_linear_8_7)" fill-opacity="0.6"/>
                        <g filter="url(#filter0_ddi_8_7)">
                            <path d="M52 511.5C52 480.296 77.2959 455 108.5 455H878.5C909.704 455 935 480.296 935 511.5V511.5C935 542.704 909.704 568 878.5 568H108.5C77.2959 568 52 542.704 52 511.5V511.5Z" fill="#D7E4E2" fill-opacity="0.6" shape-rendering="crispEdges"/>
                        </g>
                        <defs>
                            <filter id="filter0_ddi_8_7" x="50" y="442.6" width="938.4" height="165.8" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
                                <feFlood flood-opacity="0" result="BackgroundImageFix"/>
                                <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
                                <feOffset dx="27" dy="14"/>
                                <feGaussianBlur stdDeviation="13.2"/>
                                <feComposite in2="hardAlpha" operator="out"/>
                                <feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.15 0"/>
                                <feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow_8_7"/>
                                <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
                                <feOffset dx="2" dy="-2"/>
                                <feGaussianBlur stdDeviation="2"/>
                                <feComposite in2="hardAlpha" operator="out"/>
                                <feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.7 0"/>
                                <feBlend mode="normal" in2="effect1_dropShadow_8_7" result="effect2_dropShadow_8_7"/>
                                <feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
                                <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
                                <feMorphology radius="85" operator="erode" in="SourceAlpha" result="effect3_innerShadow_8_7"/>
                                <feOffset/>
                                <feGaussianBlur stdDeviation="28.9"/>
                                <feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
                                <feColorMatrix type="matrix" values="0 0 0 0 0.775641 0 0 0 0 0.775641 0 0 0 0 0.775641 0 0 0 0.25 0"/>
                                <feBlend mode="normal" in2="shape" result="effect3_innerShadow_8_7"/>
                                <feBlend mode="normal" in="effect3_innerShadow_8_7" in2="effect2_dropShadow_8_7" result="effect3_innerShadow_8_7"/>
                            </filter>
                            <linearGradient id="paint0_linear_8_7" x1="530.503" y1="312.895" x2="530.503" y2="710.265" gradientUnits="userSpaceOnUse">
                                <stop stop-color="white"/>
                                <stop offset="1" stop-color="#EE922A"/>
                            </linearGradient>
                        </defs>
                    </svg>
                    <span class="inline text-contrast font-semibold text-xl tracking-wide select-none" style="font-size: 25px;translateY(-90px); margin-top: -10px; margin-right: 15px;">Celestial Launcher</span>
                </div>
                <ProgressBar class="loading-bar" :progress="Math.min(loadingProgress, 100)" />
				<span v-if="message">{{ message }}</span>
			</div>
			<div class="gradient-bg" data-tauri-drag-region></div>
			<div class="cube-bg"></div>
			<div class="base-bg"></div>
		</div>
	</Transition>
</template>

<script setup>
import { injectLoadingState } from '@modrinth/ui'
import { ref, watch } from 'vue'

import ProgressBar from '@/components/ui/ProgressBar.vue'
import { loading_listener } from '@/helpers/events.js'

const doneLoading = ref(false)
const loadingProgress = ref(0)
const message = ref()

const MIN_DISPLAY_MS = 500
const mountedAt = Date.now()

const loading = injectLoadingState()

function onAfterLeave() {
	loading.setEnabled(true)
}

watch(
	[loading.barEnabled, loading.pending],
	([barEnabled, pending]) => {
		if (barEnabled) {
			return
		}

		if (pending) {
			loadingProgress.value = 0
			fakeLoadingIncrease()
			return
		}

		const elapsed = Date.now() - mountedAt
		const delay = Math.max(0, MIN_DISPLAY_MS - elapsed)

		setTimeout(() => {
			if (loading.pending.value) {
				return
			}
			doneLoading.value = true
		}, delay)
	},
	{ immediate: true },
)

function fakeLoadingIncrease() {
	if (loadingProgress.value < 95) {
		setTimeout(() => {
			loadingProgress.value += 2
			fakeLoadingIncrease()
		}, 5)
	}
}

loading_listener(async (e) => {
	if (e.event.type === 'directory_move') {
		loadingProgress.value = 100 * (e.fraction ?? 1)
		message.value = '正在更新应用目录...'
	} else if (e.event.type === 'checking_for_updates') {
		loadingProgress.value = 100 * (e.fraction ?? 1)
		message.value = '正在检查更新...'
	}
})
</script>

<style scoped lang="scss">
.splash-screen {
	position: fixed;
	inset: 0;
	z-index: 10000;
}

.splash-fade-leave-active {
	transition: opacity 0.3s ease-in-out;
}

.splash-fade-leave-to {
	opacity: 0;
}

.app-logo-wrapper {
	position: absolute;
	height: 100vh;
	width: 100%;

	display: flex;
	flex-direction: column;
	justify-content: center;
	align-items: center;

	gap: 1rem;

	z-index: 9998;
}

.app-logo {
	height: 6rem;
	width: fit-content;
    margin-top: -5rem;
    transform: translateY(-80px);
}

.loading-bar {
	max-width: 20rem;
}

.gradient-bg {
	position: absolute;
	height: 100vh;
	width: 100vw;
	background:
		linear-gradient(180deg, rgb(225 173 96 / 0.27) 0%, rgb(43 34 17 / 0.5) 97.29%),
		linear-gradient(0deg, rgb(22 28 26 / 0.64), rgb(22 28 26 / 0.64));
	z-index: 9997;
}

.cube-bg {
	position: absolute;

	left: 50%;
	top: 50%;
	transform: translate(-50%, -50%);

	width: 180vw;
	height: 180vh;
	opacity: 0.8;
	background: #16181c url('@/assets/loading/cube.png') center no-repeat;
	background-size: contain;

	z-index: 9996;
}

.base-bg {
	position: absolute;
	top: 0;
	left: 0;
	width: 100%;
	height: 100%;
	background: var(--color-bg);
	z-index: 9995;
}
</style>
