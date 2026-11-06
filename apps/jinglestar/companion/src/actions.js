// Actions : chacune envoie une commande OSC à JingleStar.

const { JINGLE_SLOTS } = require('./state')

const MODULE_DB = [-60, 10]
const JINGLE_DB = [-40, 10]
const MAX_MODULES = 4

const clamp = (v, [lo, hi]) => Math.min(hi, Math.max(lo, v))

function moduleChoices(state) {
	const known = new Map((state?.modules ?? []).map((m) => [m.number, m]))
	const choices = []
	for (let n = 1; n <= MAX_MODULES; n++) {
		const m = known.get(n)
		choices.push({ id: n, label: m ? `${n} · ${m.name}` : `Module ${n}` })
	}
	return choices
}

function moduleOption(state) {
	return { type: 'dropdown', id: 'module', label: 'Module', default: 1, choices: moduleChoices(state) }
}

function jingleOption() {
	const choices = []
	for (let s = 1; s <= JINGLE_SLOTS; s++) choices.push({ id: s, label: `Jingle ${s}` })
	return { type: 'dropdown', id: 'jingle', label: 'Jingle', default: 1, choices }
}

const SWITCH = {
	type: 'dropdown',
	id: 'state',
	label: 'État',
	default: 'toggle',
	choices: [
		{ id: 'toggle', label: 'Basculer' },
		{ id: 'on', label: 'Activer' },
		{ id: 'off', label: 'Désactiver' },
	],
}

/** État voulu d'une option : imposé, ou l'inverse de l'état actuel. */
function wanted(opt, current) {
	return opt === 'on' ? true : opt === 'off' ? false : !current
}

function getActions(self) {
	const state = self.state
	const mod = moduleOption(state)
	const jingle = jingleOption()
	const m = (e) => Number(e.options.module)
	const j = (e) => Number(e.options.jingle)
	const send = (path, args) => self.send(path, args)
	const playlistOf = (n) => self.module(n)?.playlist
	const trigger = (name, path, opts = [mod]) => ({
		name,
		options: opts,
		callback: (e) => send(path(e)),
	})

	return {
		stop_all: { name: 'Tout arrêter', options: [], callback: () => send('/stop_all') },
		go_first: { name: 'Cuelist : GO (premier module GO)', options: [], callback: () => send('/go') },

		module_stop: trigger('Module : tout arrêter', (e) => `/module/${m(e)}/stop`),
		module_volume: {
			name: 'Module : volume (dB)',
			options: [mod, { type: 'number', id: 'db', label: 'Volume (dB)', default: 0, min: MODULE_DB[0], max: MODULE_DB[1], step: 0.5 }],
			callback: (e) => send(`/module/${m(e)}/volume`, [{ type: 'f', value: clamp(Number(e.options.db), MODULE_DB) }]),
		},
		module_volume_step: {
			name: 'Module : volume +/−',
			options: [mod, { type: 'number', id: 'step', label: 'Pas (dB, négatif pour baisser)', default: 1, min: -20, max: 20, step: 0.5 }],
			callback: (e) => {
				const cur = self.module(m(e))?.volume ?? 0
				send(`/module/${m(e)}/volume`, [{ type: 'f', value: clamp(cur + Number(e.options.step), MODULE_DB) }])
			},
		},
		module_mode: {
			name: 'Module : type',
			options: [
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
			callback: (e) => send(`/module/${m(e)}/mode`, [{ type: 's', value: e.options.mode }]),
		},

		jingle_toggle: trigger('Jingle : lancer / arrêter', (e) => `/module/${m(e)}/jingle/${j(e)}/toggle`, [mod, jingle]),
		jingle_play: trigger('Jingle : lancer', (e) => `/module/${m(e)}/jingle/${j(e)}/play`, [mod, jingle]),
		jingle_stop: trigger('Jingle : arrêter', (e) => `/module/${m(e)}/jingle/${j(e)}/stop`, [mod, jingle]),
		jingle_volume: {
			name: 'Jingle : volume (dB)',
			options: [mod, jingle, { type: 'number', id: 'db', label: 'Volume (dB)', default: 0, min: JINGLE_DB[0], max: JINGLE_DB[1], step: 0.5 }],
			callback: (e) => send(`/module/${m(e)}/jingle/${j(e)}/volume`, [{ type: 'f', value: clamp(Number(e.options.db), JINGLE_DB) }]),
		},
		jingle_volume_step: {
			name: 'Jingle : volume +/−',
			options: [mod, jingle, { type: 'number', id: 'step', label: 'Pas (dB, négatif pour baisser)', default: 1, min: -20, max: 20, step: 0.5 }],
			callback: (e) => {
				const cur = self.module(m(e))?.jingles[j(e) - 1]?.volume ?? 0
				send(`/module/${m(e)}/jingle/${j(e)}/volume`, [{ type: 'f', value: clamp(cur + Number(e.options.step), JINGLE_DB) }])
			},
		},

		playlist_play: trigger('Playlist : lecture', (e) => `/module/${m(e)}/playlist/play`),
		playlist_stop: trigger('Playlist : arrêt', (e) => `/module/${m(e)}/playlist/stop`),
		playlist_pause: {
			name: 'Playlist : pause / reprise',
			options: [mod, SWITCH],
			callback: (e) => {
				if (e.options.state === 'toggle') send(`/module/${m(e)}/playlist/pause`)
				else send(`/module/${m(e)}/playlist/paused`, [e.options.state === 'on'])
			},
		},
		playlist_next: trigger('Playlist : morceau suivant', (e) => `/module/${m(e)}/playlist/next`),
		playlist_previous: trigger('Playlist : morceau précédent', (e) => `/module/${m(e)}/playlist/previous`),
		playlist_track: {
			name: 'Playlist : lancer le morceau n°',
			options: [mod, { type: 'number', id: 'track', label: 'Morceau', default: 1, min: 1, max: 9999 }],
			callback: (e) => send(`/module/${m(e)}/playlist/track`, [{ type: 'i', value: Number(e.options.track) }]),
		},
		playlist_shuffle: {
			name: 'Playlist : lecture aléatoire',
			options: [mod, SWITCH],
			callback: (e) => send(`/module/${m(e)}/playlist/shuffle`, [wanted(e.options.state, playlistOf(m(e))?.shuffle)]),
		},
		playlist_loop: {
			name: 'Playlist : boucle',
			options: [mod, SWITCH],
			callback: (e) => send(`/module/${m(e)}/playlist/loop`, [wanted(e.options.state, playlistOf(m(e))?.loop)]),
		},

		cue_go: trigger('Cuelist : GO', (e) => `/module/${m(e)}/go`),
		cue_next: trigger('Cuelist : cue suivant en attente', (e) => `/module/${m(e)}/cue/next`),
		cue_previous: trigger('Cuelist : cue précédent en attente', (e) => `/module/${m(e)}/cue/previous`),
		cue_pause: {
			name: 'Cuelist : pause / reprise',
			options: [mod, SWITCH],
			callback: (e) => {
				if (e.options.state === 'toggle') send(`/module/${m(e)}/cue/pause`)
				else send(`/module/${m(e)}/cue/paused`, [e.options.state === 'on'])
			},
		},
		cue_standby: {
			name: 'Cuelist : mettre le cue n° en attente',
			options: [mod, { type: 'number', id: 'cue', label: 'Cue', default: 1, min: 1, max: 9999 }],
			callback: (e) => send(`/module/${m(e)}/cue/standby`, [{ type: 'i', value: Number(e.options.cue) }]),
		},
	}
}

module.exports = { getActions, moduleOption, jingleOption, MAX_MODULES }
