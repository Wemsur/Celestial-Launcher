<template>
	<div class="page-header-actions flex flex-wrap items-center gap-2">
		<slot />
	</div>
</template>

<style scoped>
/*
 * `page-header` is the container named by a header with stackActionsWhenNarrow set.
 * Once it is too narrow to keep the actions next to the title without squeezing it,
 * they take a row of their own below it: the primary action stretches across the row
 * and any trailing icon buttons (e.g. the overflow menu) keep their size at its end.
 */
@container page-header (width < 800px) {
	.page-header-actions {
		@apply w-full;
	}

	.page-header-actions > :deep(*:first-child) {
		@apply flex-1;
	}
}
</style>
