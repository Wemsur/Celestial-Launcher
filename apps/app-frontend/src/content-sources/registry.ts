/**
 * The live providers of every loaded content source.
 *
 * The source *list* (ids, manifests, enabled state) comes from the backend —
 * that is what the source manager renders. This registry is the other half: the
 * JavaScript providers that only exist while a source is loaded, which the
 * discover page calls. A source whose plugin threw is absent here and has an
 * entry in {@link contentSourceErrors} instead.
 */

import { shallowRef } from 'vue'

import type { ContentSourceProvider, SourceSummary } from './types'

const providers = shallowRef(new Map<string, ContentSourceProvider>())
const errors = shallowRef(new Map<string, string>())
const summaries = shallowRef<SourceSummary[]>([])

/** The installed sources as the backend reports them. Reactive. */
export const contentSourceSummaries = summaries

/** Replace the installed-source list; called by the loader on each load. */
export function setContentSourceSummaries(next: SourceSummary[]): void {
	summaries.value = next
}

/** Registration is keyed by source id; re-registering replaces (hot reload). */
export function registerSourceProvider(
	sourceId: string,
	provider: ContentSourceProvider,
): void {
	const next = new Map(providers.value)
	next.set(sourceId, provider)
	providers.value = next
}

export function setSourceLoadError(sourceId: string, message: string): void {
	const next = new Map(errors.value)
	next.set(sourceId, message)
	errors.value = next
}

/** Drop every provider and error; called before a reload rebuilds them. */
export function clearSourceProviders(): void {
	providers.value = new Map()
	errors.value = new Map()
}

/** Providers of every loaded source. Reactive. */
export const contentSourceProviders = providers

/** Load failures, by source id. Reactive. */
export const contentSourceErrors = errors

/** The provider for a source, or undefined when it is not loaded. */
export function sourceProvider(
	sourceId: string,
): ContentSourceProvider | undefined {
	return providers.value.get(sourceId)
}
