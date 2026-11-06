// Variables : noms, numéros et temps restants, à afficher sur les touches.

const { formatTime } = require('./state')

const MODES = { jingle: 'Jingles', playlist: 'Playlist', go: 'Cuelist', text: 'Texte' }

function getVariableDefinitions(state) {
	const defs = [{ variableId: 'connected', name: 'Connecté à JingleStar (oui / non)' }]
	for (const m of state?.modules ?? []) {
		const p = `module_${m.number}`
		const N = `Module ${m.number}`
		defs.push(
			{ variableId: `${p}_name`, name: `${N} : nom` },
			{ variableId: `${p}_mode`, name: `${N} : type` },
			{ variableId: `${p}_volume`, name: `${N} : volume (dB)` },
		)
		for (const j of m.jingles) {
			const q = `${p}_jingle_${j.number}`
			const J = `${N}, jingle ${j.number}`
			defs.push(
				{ variableId: `${q}_name`, name: `${J} : nom` },
				{ variableId: `${q}_volume`, name: `${J} : volume (dB)` },
				{ variableId: `${q}_remaining`, name: `${J} : temps restant` },
			)
		}
		if (m.playlist) {
			defs.push(
				{ variableId: `${p}_track`, name: `${N} : numéro du morceau` },
				{ variableId: `${p}_tracks`, name: `${N} : nombre de morceaux` },
				{ variableId: `${p}_track_title`, name: `${N} : titre du morceau` },
				{ variableId: `${p}_track_remaining`, name: `${N} : temps restant du morceau` },
			)
		}
		if (m.cue) {
			defs.push(
				{ variableId: `${p}_cue_standby`, name: `${N} : numéro du cue en attente` },
				{ variableId: `${p}_cue_name`, name: `${N} : nom du cue en attente` },
				{ variableId: `${p}_cue_count`, name: `${N} : nombre de cues` },
				{ variableId: `${p}_cue_playing`, name: `${N} : nombre de cues qui sonnent` },
				{ variableId: `${p}_cue_playing_name`, name: `${N} : nom du cue qui sonne` },
				{ variableId: `${p}_cue_remaining`, name: `${N} : temps restant du cue` },
			)
		}
	}
	return defs
}

const db = (v) => (Math.round(v * 10) / 10).toFixed(1)

function getVariableValues(state) {
	const v = { connected: state ? 'oui' : 'non' }
	for (const m of state?.modules ?? []) {
		const p = `module_${m.number}`
		v[`${p}_name`] = m.name
		v[`${p}_mode`] = MODES[m.mode] ?? m.mode
		v[`${p}_volume`] = db(m.volume)
		for (const j of m.jingles) {
			const q = `${p}_jingle_${j.number}`
			v[`${q}_name`] = j.name
			v[`${q}_volume`] = db(j.volume)
			v[`${q}_remaining`] = j.playing ? formatTime(j.remaining) : ''
		}
		if (m.playlist) {
			const pl = m.playlist
			v[`${p}_track`] = pl.track === null ? '' : String(pl.track)
			v[`${p}_tracks`] = String(pl.tracks)
			v[`${p}_track_title`] = pl.title
			v[`${p}_track_remaining`] = pl.playing ? formatTime(pl.remaining) : ''
		}
		if (m.cue) {
			const c = m.cue
			v[`${p}_cue_standby`] = c.standby === null ? 'FIN' : String(c.standby)
			v[`${p}_cue_name`] = c.name
			v[`${p}_cue_count`] = String(c.count)
			v[`${p}_cue_playing`] = String(c.playing)
			v[`${p}_cue_playing_name`] = c.playingName
			v[`${p}_cue_remaining`] = c.playing > 0 ? formatTime(c.remaining) : ''
		}
	}
	return v
}

module.exports = { getVariableDefinitions, getVariableValues }
