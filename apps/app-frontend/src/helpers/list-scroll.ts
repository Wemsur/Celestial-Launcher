/**
 * Preserving the discover list's place across a split-view switch.
 *
 * The list scrolls in a *different* element depending on the layout: at full
 * width it is the app viewport, and in split view it is the list pane, which
 * scrolls itself so the two panes get their own scrollbars. Those are separate
 * DOM nodes, so the switch moves the list to a fresh container that starts at
 * zero — the position is dropped, not merely mis-scaled.
 *
 * A pixel offset would not survive the switch either: the same list is narrower
 * in split view, so cards reflow to different heights and the same pixel is a
 * different place in the list. What does survive is the *fraction* of the
 * scrollable height — the reader was a third of the way down, so they are put a
 * third of the way down — which is why a ratio is what is kept.
 *
 * The ratio is recorded on every scroll rather than read at the switch, because
 * the switch re-renders before anything can read the old container: by the time
 * a watcher fires, the element that was scrolled has already given up scrolling
 * and reset to zero. Continuous capture makes the timing irrelevant.
 *
 * Module-level, because the container is not the component's: at full width the
 * list is scrolled by the app viewport, which the discover page does not own.
 * One slot is enough — there is one discover list.
 */

let savedRatio: number | null = null

/** The ratio of an element's scroll, 0 at the top and 1 at the bottom. */
function scrollRatio(el: Element): number {
	const scrollable = el.scrollHeight - el.clientHeight
	if (scrollable <= 0) return 0
	return el.scrollTop / scrollable
}

/** Record where the list is, from the element scrolling it right now. */
export function rememberListScroll(el: Element | null | undefined): void {
	if (!el) return
	savedRatio = scrollRatio(el)
}

/**
 * Forget the recorded position, so the next restore leaves the list at the top.
 * Called when the list is reached fresh — a new navigation into discover, or a
 * switch back to full width — so an old position cannot resurface later.
 */
export function clearListScroll(): void {
	savedRatio = null
}

/**
 * Put the list back where it was, on whichever element scrolls it now. Does
 * nothing when nothing has been recorded, so a first visit stays at the top.
 *
 * The write is spread over several frames rather than made once: the list is
 * narrower in its new home, so its cards have not reflowed to their new heights
 * yet — and when the switch remounts the list, its results have not arrived at
 * all, leaving it too short to scroll to the recorded fraction. Applying the
 * ratio on each frame tracks the height as it settles, and stops once it stops
 * changing so a long settle cannot fight the reader's own scrolling.
 */
export function restoreListScroll(el: Element | null | undefined): void {
	if (savedRatio === null || !el) return
	const ratio = savedRatio

	let frames = 0
	let lastScrollable = -1
	let stable = 0

	const apply = () => {
		const scrollable = el.scrollHeight - el.clientHeight
		el.scrollTop = ratio * scrollable

		stable = scrollable === lastScrollable ? stable + 1 : 0
		lastScrollable = scrollable

		// Enough frames for images and late results to land, but not an unbounded
		// loop that keeps yanking the list back while the reader scrolls it.
		if (stable < 3 && frames < 60) {
			frames += 1
			requestAnimationFrame(apply)
		}
	}

	requestAnimationFrame(apply)
}
