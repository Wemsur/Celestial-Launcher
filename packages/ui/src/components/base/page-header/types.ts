import type { Component, HTMLAttributes } from 'vue'
import type { RouteLocationRaw } from 'vue-router'

export type PageHeaderTarget = string | RouteLocationRaw
export type PageHeaderClass = HTMLAttributes['class']

export type PageHeaderClickHandler = (event: MouseEvent) => void | Promise<void>

export type PageHeaderIconProps = {
	icon?: Component
	iconProps?: Record<string, unknown>
	iconClass?: PageHeaderClass
}

export type PageHeaderInteractiveProps = {
	tooltip?: string
	ariaLabel?: string
	to?: PageHeaderTarget
	action?: PageHeaderClickHandler
	disabled?: boolean
}

export type PageHeaderMetadataItemProps = PageHeaderIconProps & PageHeaderInteractiveProps

export type PageHeaderProps = {
	title: string
	summary?: string | null
	headerClass?: PageHeaderClass
	rowClass?: PageHeaderClass
	mainClass?: PageHeaderClass
	titleClass?: PageHeaderClass
	truncateTitle?: boolean
	divider?: boolean
	bottomPadding?: boolean
	disableLineClamp?: boolean
	/**
	 * Lets the header react to its own width instead of the viewport's: when it gets
	 * narrower than 800px the actions move to a full-width row of their own, with the
	 * primary action stretched. Useful wherever the header lives in a pane rather than
	 * the full page (e.g. the launcher's split view).
	 */
	stackActionsWhenNarrow?: boolean
}
