// Lecture de l'arborescence OSCQuery publiée par JingleStar.
// Fonctions pures : pas de dépendance à Companion, testables seules.

const JINGLE_SLOTS = 12

/** Première valeur d'une feuille OSCQuery, ou `undefined`. */
function val(node) {
	return Array.isArray(node?.VALUE) ? node.VALUE[0] : undefined
}

function child(node, name) {
	return node?.CONTENTS?.[name]
}

/**
 * Cherche le dossier qui contient « stop_all » : la racine des commandes, quel que soit
 * le préfixe réglé dans JingleStar. Renvoie [chemin, nœud].
 */
function findBase(root) {
	const queue = [['', root]]
	while (queue.length) {
		const [path, node] = queue.shift()
		const contents = node?.CONTENTS
		if (!contents) continue
		if (contents.stop_all) return [path, node]
		if (path.split('/').length > 4) continue
		for (const [name, sub] of Object.entries(contents)) queue.push([`${path}/${name}`, sub])
	}
	return [null, null]
}

/** Couleur « #rrggbb » ou [r, g, b] → [r, g, b]. */
function parseColor(v) {
	if (Array.isArray(v) && v.length >= 3) return v.slice(0, 3).map((c) => Number(c) | 0)
	if (typeof v === 'string') {
		const m = /^#?([0-9a-f]{6})$/i.exec(v.trim())
		if (m) {
			const n = parseInt(m[1], 16)
			return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
		}
	}
	return null
}

function num(v) {
	return typeof v === 'number' && Number.isFinite(v) ? v : null
}

/** Temps restant : JingleStar publie -1 quand il n'y en a pas. */
function time(v) {
	const n = num(v)
	return n !== null && n >= 0 ? n : null
}

/**
 * État simplifié de JingleStar. Modules et jingles gardent leur numéro (à partir de 1).
 * Les champs absents des anciennes versions de JingleStar valent `null`.
 */
function parseTree(root) {
	const [prefix, base] = findBase(root)
	if (base === null) return null
	const modules = []
	const folder = child(base, 'module')?.CONTENTS ?? {}
	const numbers = Object.keys(folder)
		.map(Number)
		.filter((n) => Number.isInteger(n) && n >= 1)
		.sort((a, b) => a - b)
	for (const n of numbers) {
		const m = folder[String(n)]
		const mod = {
			number: n,
			name: String(val(child(m, 'name')) ?? `Module ${n}`),
			mode: String(val(child(m, 'mode')) ?? ''),
			volume: num(val(child(m, 'volume'))) ?? 0,
			jingles: [],
			playlist: null,
			cue: null,
		}
		const jingles = child(m, 'jingle')
		if (jingles) {
			for (let s = 1; s <= JINGLE_SLOTS; s++) {
				const j = child(jingles, String(s))
				const name = String(val(child(j, 'name')) ?? '')
				mod.jingles.push({
					number: s,
					name,
					loaded: name !== '',
					volume: num(val(child(j, 'volume'))) ?? 0,
					playing: Boolean(val(child(j, 'playing'))),
					color: parseColor(val(child(j, 'color'))),
					remaining: time(val(child(j, 'remaining'))),
				})
			}
		}
		const pl = child(m, 'playlist')
		if (pl) {
			const track = num(val(child(pl, 'track'))) ?? 0
			mod.playlist = {
				track: track >= 1 ? track : null,
				tracks: num(val(child(pl, 'tracks'))) ?? 0,
				title: String(val(child(pl, 'title')) ?? ''),
				playing: Boolean(val(child(pl, 'playing'))),
				paused: Boolean(val(child(pl, 'paused'))),
				shuffle: Boolean(val(child(pl, 'shuffle'))),
				loop: Boolean(val(child(pl, 'loop'))),
				remaining: time(val(child(pl, 'remaining'))),
			}
		}
		const cue = child(m, 'cue')
		if (cue) {
			const standby = num(val(child(cue, 'standby'))) ?? 0
			mod.cue = {
				standby: standby >= 1 ? standby : null,
				name: String(val(child(cue, 'name')) ?? ''),
				count: num(val(child(cue, 'count'))) ?? 0,
				playing: num(val(child(cue, 'playing'))) ?? 0,
				playingName: String(val(child(cue, 'playing_name')) ?? ''),
				remaining: time(val(child(cue, 'remaining'))),
				paused: Boolean(val(child(cue, 'paused'))),
			}
		}
		modules.push(mod)
	}
	return { prefix, modules }
}

/** Ce qui change les listes d'actions, de variables et de presets. */
function structureKey(state) {
	return state ? state.prefix + '|' + state.modules.map((m) => `${m.number}:${m.mode}`).join(',') : ''
}

/** Secondes → « m:ss » ; vide quand rien ne joue. */
function formatTime(s) {
	if (s === null || s === undefined || !Number.isFinite(s)) return ''
	const t = Math.max(0, Math.ceil(s))
	return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`
}

module.exports = { JINGLE_SLOTS, parseTree, structureKey, formatTime, findBase, parseColor }
