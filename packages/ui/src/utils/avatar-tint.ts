/**
 * The colour `Avatar` gives an icon-less entity, exposed so other surfaces can
 * match it exactly.
 *
 * `Avatar` derives a hue from a seed string (usually an id) and mixes a sliver of
 * it into the button background. Anything that wants to sit flush against such an
 * avatar — a card using the same tint as its own background, say — has to agree on
 * the hue down to the sign, so the maths lives here rather than being copied.
 */

/**
 * Deliberately the classic `(h << 5) - h + c` string hash, kept bit-for-bit: the
 * hue is baked into how every existing instance card already looks, so a "better"
 * hash would silently recolour the whole library.
 */
function hashSeed(seed: string): number {
	let hash = 0
	for (let i = 0, len = seed.length; i < len; i++) {
		const chr = seed.charCodeAt(i)
		hash = (hash << 5) - hash + chr
		hash |= 0
	}
	return hash
}

/**
 * Hue in degrees for a seed. May be negative — `hashSeed` returns a signed 32-bit
 * integer and CSS wraps negative hues, so the sign is part of the result and must
 * not be normalised away.
 */
export function avatarTintHue(seed: string): number {
	return hashSeed(seed) % 360
}

/** The `oklch()` colour `Avatar` mixes in for a seed. */
export function avatarTintColor(seed: string): string {
	return `oklch(50% 75% ${avatarTintHue(seed)})`
}

/**
 * The exact background an icon-less `Avatar` paints, as a CSS colour.
 *
 * The 100%/5% weights are what `Avatar`'s stylesheet uses; CSS normalises them to
 * roughly 95%/5%, which is why the tint reads as a faint wash rather than a colour.
 */
export function avatarTintBackground(seed: string): string {
	return `color-mix(in oklch, var(--color-button-bg) 100%, ${avatarTintColor(seed)} 5%)`
}
