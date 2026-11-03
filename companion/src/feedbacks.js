// Retours d'état sur les touches.

const { combineRgb } = require('@companion-module/base')
const { moduleOption, jingleOption } = require('./actions')
const { progressBar, stereoMeter } = require('./graphics')

const RED = combineRgb(200, 0, 0)
const GREEN = combineRgb(0, 150, 0)
const ORANGE = combineRgb(230, 120, 0)
const BLUE = combineRgb(0, 90, 200)
const WHITE = combineRgb(255, 255, 255)
const BLACK = combineRgb(0, 0, 0)

function getFeedbacks(self) {
	const mod = moduleOption(self.state)
	const jingle = jingleOption()
	const m = (fb) => self.module(Number(fb.options.module))
	const j = (fb) => m(fb)?.jingles[Number(fb.options.jingle) - 1]
	const bool = (name, description, color, options, check) => ({
		type: 'boolean',
		name,
		description,
		defaultStyle: { bgcolor: color, color: WHITE },
		options,
		callback: check,
	})

	return {
		jingle_playing: bool('Jingle : joue', 'Le jingle est en cours de lecture', RED, [mod, jingle], (fb) => Boolean(j(fb)?.playing)),
		jingle_loaded: bool('Jingle : chargé', 'Un son est placé sur ce jingle', BLUE, [mod, jingle], (fb) => Boolean(j(fb)?.loaded)),
		jingle_color: {
			type: 'advanced',
			name: 'Jingle : couleur de JingleStar',
			description: 'Reprend la couleur du jingle (JingleStar 0.1.3 et suivants)',
			options: [mod, jingle],
			callback: (fb) => {
				const c = j(fb)?.color
				if (!c) return {}
				// Texte noir sur les couleurs claires, comme dans JingleStar.
				const light = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2] > 150
				return { bgcolor: combineRgb(c[0], c[1], c[2]), color: light ? BLACK : WHITE }
			},
		},
		time_warning: bool(
			'Temps restant : alerte',
			'Le son qui joue (jingle, morceau ou cue) a moins de N secondes restantes (JingleStar 0.1.3 et suivants)',
			ORANGE,
			[
				mod,
				{
					type: 'dropdown',
					id: 'source',
					label: 'Son',
					default: 'jingle',
					choices: [
						{ id: 'jingle', label: 'Jingle' },
						{ id: 'playlist', label: 'Morceau de la playlist' },
						{ id: 'cue', label: 'Cue de la cuelist' },
					],
				},
				{ ...jingle, isVisible: (o) => o.source === 'jingle' },
				{ type: 'number', id: 'seconds', label: 'Secondes', default: 10, min: 1, max: 600 },
			],
			(fb) => {
				const mm = m(fb)
				const r =
					fb.options.source === 'playlist'
						? mm?.playlist?.playing && mm.playlist.remaining
						: fb.options.source === 'cue'
							? mm?.cue?.playing && mm.cue.remaining
							: j(fb)?.playing && j(fb).remaining
				return typeof r === 'number' && r <= Number(fb.options.seconds)
			},
		),
		progress_bar: {
			type: 'advanced',
			name: 'Barre de lecture',
			description: 'Barre qui avance en bas de la touche pendant la lecture (jingle, morceau ou cue)',
			options: [
				mod,
				{
					type: 'dropdown',
					id: 'source',
					label: 'Son',
					default: 'cue',
					choices: [
						{ id: 'jingle', label: 'Jingle' },
						{ id: 'playlist', label: 'Morceau de la playlist' },
						{ id: 'cue', label: 'Cue de la cuelist' },
					],
				},
				{ ...jingle, isVisible: (o) => o.source === 'jingle' },
				{ type: 'colorpicker', id: 'color', label: 'Couleur', default: combineRgb(255, 255, 255) },
				{ type: 'number', id: 'height', label: 'Épaisseur (pixels)', default: 8, min: 2, max: 72 },
			],
			callback: (fb) => {
				const mm = m(fb)
				const p =
					fb.options.source === 'playlist'
						? mm?.playlist?.progress
						: fb.options.source === 'cue'
							? mm?.cue?.progress
							: j(fb)?.progress
				if (typeof p !== 'number') return {}
				const width = fb.image?.width ?? 72
				const full = fb.image?.height ?? 72
				const height = Math.min(full, Math.max(2, Number(fb.options.height) || 8))
				return {
					imageBuffer: progressBar(width, height, p, Number(fb.options.color)),
					imageBufferEncoding: { pixelFormat: 'RGBA' },
					imageBufferPosition: { x: 0, y: full - height, width, height },
				}
			},
		},
		module_meter: {
			type: 'advanced',
			name: 'Module : vumètre',
			description: 'Niveau de sortie du module (gauche et droite), envoyé en continu par JingleStar',
			options: [
				mod,
				{
					type: 'dropdown',
					id: 'layout',
					label: 'Place',
					default: 'side',
					choices: [
						{ id: 'side', label: 'Bande sur le bord droit' },
						{ id: 'full', label: 'Toute la touche' },
					],
				},
			],
			callback: (fb) => {
				const width = fb.image?.width ?? 72
				const height = fb.image?.height ?? 72
				const w = fb.options.layout === 'full' ? width : Math.max(8, Math.round(width / 5))
				return {
					imageBuffer: stereoMeter(w, height, self.meter(Number(fb.options.module))),
					imageBufferEncoding: { pixelFormat: 'RGBA' },
					imageBufferPosition: { x: width - w, y: 0, width: w, height },
				}
			},
		},

		playlist_playing: bool('Playlist : en lecture', 'La playlist joue (hors pause)', GREEN, [mod], (fb) => {
			const p = m(fb)?.playlist
			return Boolean(p?.playing && !p.paused)
		}),
		playlist_paused: bool('Playlist : en pause', 'La playlist est en pause', ORANGE, [mod], (fb) => Boolean(m(fb)?.playlist?.paused)),
		playlist_shuffle: bool('Playlist : aléatoire', 'La lecture aléatoire est active', BLUE, [mod], (fb) => Boolean(m(fb)?.playlist?.shuffle)),
		playlist_loop: bool('Playlist : boucle', 'La boucle sur la liste est active', BLUE, [mod], (fb) => Boolean(m(fb)?.playlist?.loop)),

		cue_playing: bool('Cuelist : un cue sonne', 'Au moins un cue de la cuelist joue', GREEN, [mod], (fb) => (m(fb)?.cue?.playing ?? 0) > 0),
		cue_paused: bool('Cuelist : en pause', 'Les cues qui sonnent sont en pause', ORANGE, [mod], (fb) => Boolean(m(fb)?.cue?.paused)),
		cue_end: bool('Cuelist : fin atteinte', 'Plus aucun cue en attente', combineRgb(80, 80, 80), [mod], (fb) => {
			const c = m(fb)?.cue
			return Boolean(c && c.standby === null)
		}),

		module_mode: bool(
			'Module : type',
			'Le module est du type choisi',
			BLUE,
			[
				mod,
				{
					type: 'dropdown',
					id: 'mode',
					label: 'Type',
					default: 'jingle',
					choices: [
						{ id: 'jingle', label: 'Jingles' },
						{ id: 'playlist', label: 'Playlist' },
						{ id: 'go', label: 'Cuelist' },
						{ id: 'text', label: 'Texte' },
					],
				},
			],
			(fb) => m(fb)?.mode === fb.options.mode,
		),
	}
}

module.exports = { getFeedbacks, colors: { RED, GREEN, ORANGE, BLUE, WHITE, BLACK } }
