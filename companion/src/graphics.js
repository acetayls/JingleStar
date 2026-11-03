// Petits dessins sur les touches : barre de lecture et vumètre (images RGBA brutes).

const METER_FLOOR = -60

function image(width, height) {
	return new Uint8Array(width * height * 4)
}

function fill(img, width, x0, y0, x1, y1, [r, g, b, a]) {
	for (let y = y0; y < y1; y++) {
		for (let x = x0; x < x1; x++) {
			const i = (y * width + x) * 4
			img[i] = r
			img[i + 1] = g
			img[i + 2] = b
			img[i + 3] = a
		}
	}
}

/** Couleur Companion (entier 0xRRGGBB) → [r, g, b, a]. */
function rgba(color, a = 255) {
	return [(color >> 16) & 255, (color >> 8) & 255, color & 255, a]
}

/** Barre horizontale remplie à `fraction` (0 à 1). */
function progressBar(width, height, fraction, color) {
	const img = image(width, height)
	fill(img, width, 0, 0, width, height, [0, 0, 0, 160])
	const w = Math.round(Math.min(1, Math.max(0, fraction)) * width)
	fill(img, width, 0, 0, w, height, rgba(color))
	return img
}

/** Hauteur (0 à 1) d'un niveau en dB sur l'échelle du vumètre. */
function meterFraction(db) {
	if (!Number.isFinite(db) || db <= METER_FLOOR) return 0
	return Math.min(1, (db - METER_FLOOR) / -METER_FLOOR)
}

/** Couleur d'une graduation selon son niveau : vert, puis jaune à -18 dB, rouge à -6 dB. */
function zoneColor(fraction) {
	const db = METER_FLOOR + fraction * -METER_FLOOR
	if (db >= -6) return [230, 30, 30, 255]
	if (db >= -18) return [230, 200, 0, 255]
	return [30, 200, 60, 255]
}

/** Vumètre stéréo vertical : deux colonnes (gauche, droite). */
function stereoMeter(width, height, [left, right]) {
	const img = image(width, height)
	fill(img, width, 0, 0, width, height, [0, 0, 0, 170])
	const gap = Math.max(1, Math.round(width / 10))
	const col = Math.floor((width - 3 * gap) / 2)
	;[left, right].forEach((db, c) => {
		const x0 = gap + c * (col + gap)
		const lit = Math.round(meterFraction(db) * height)
		for (let y = height - lit; y < height; y++) {
			fill(img, width, x0, y, x0 + col, y + 1, zoneColor((height - y) / height))
		}
	})
	return img
}

module.exports = { progressBar, stereoMeter, meterFraction }
