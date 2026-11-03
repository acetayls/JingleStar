// Presets : touches prêtes à glisser sur le Stream Deck, selon les modules du projet ouvert.

const { combineRgb } = require('@companion-module/base')
const { colors } = require('./feedbacks')

const GREY = combineRgb(40, 40, 40)

/** Options de la barre de lecture en bas d'une touche. */
const bar = (source, opts) => ({ ...opts, source, color: colors.WHITE, height: 8 })

function button(category, name, text, actions, feedbacks = [], bgcolor = GREY) {
	return {
		type: 'button',
		category,
		name,
		style: { text, size: 'auto', color: colors.WHITE, bgcolor },
		steps: [{ down: actions.map(([actionId, options = {}]) => ({ actionId, options })), up: [] }],
		feedbacks: feedbacks.map(([feedbackId, options = {}, style]) => (style ? { feedbackId, options, style } : { feedbackId, options })),
	}
}

function getPresets(state, label) {
	const v = (id) => `$(${label}:${id})`
	const presets = {}
	const general = 'Général'
	presets.stop_all = button(general, 'Tout arrêter', 'TOUT\nARRÊTER', [['stop_all']], [], colors.RED)
	presets.go_first = button(general, 'GO (premier module GO)', 'GO', [['go_first']], [], colors.GREEN)

	for (const m of state?.modules ?? []) {
		const module = m.number
		const p = `module_${module}`
		const cat = `Module ${module} · ${m.name}`
		const mod = { module }

		presets[`${p}_stop`] = button(cat, 'Arrêter le module', `STOP\n${v(`${p}_name`)}`, [['module_stop', mod]], [], colors.RED)
		presets[`${p}_vol_up`] = button(cat, 'Volume +1 dB', `VOL +\n${v(`${p}_volume`)} dB`, [['module_volume_step', { ...mod, step: 1 }]])
		presets[`${p}_vol_down`] = button(cat, 'Volume −1 dB', `VOL −\n${v(`${p}_volume`)} dB`, [['module_volume_step', { ...mod, step: -1 }]])

		for (const j of m.jingles) {
			const opts = { module, jingle: j.number }
			const q = `${p}_jingle_${j.number}`
			presets[`${q}_toggle`] = button(
				cat,
				`Jingle ${j.number}`,
				`${v(`${q}_name`)}\n${v(`${q}_remaining`)}`,
				[['jingle_toggle', opts]],
				[
					['jingle_color', opts],
					['jingle_playing', opts, { bgcolor: colors.RED, color: colors.WHITE }],
					['time_warning', { ...opts, source: 'jingle', seconds: 5 }, { bgcolor: colors.ORANGE, color: colors.WHITE }],
					['progress_bar', bar('jingle', opts)],
				],
			)
		}

		if (m.playlist) {
			const fbPlaying = ['playlist_playing', mod, { bgcolor: colors.GREEN, color: colors.WHITE }]
			presets[`${p}_play`] = button(cat, 'Lecture', 'LECTURE', [['playlist_play', mod]], [fbPlaying])
			presets[`${p}_pause`] = button(cat, 'Pause / reprise', 'PAUSE', [['playlist_pause', { ...mod, state: 'toggle' }]], [
				['playlist_paused', mod, { bgcolor: colors.ORANGE, color: colors.WHITE }],
			])
			presets[`${p}_pl_stop`] = button(cat, 'Arrêt', 'ARRÊT', [['playlist_stop', mod]])
			presets[`${p}_prev`] = button(cat, 'Morceau précédent', '⏮', [['playlist_previous', mod]])
			presets[`${p}_next`] = button(cat, 'Morceau suivant', '⏭', [['playlist_next', mod]])
			presets[`${p}_shuffle`] = button(cat, 'Aléatoire', 'ALÉA', [['playlist_shuffle', { ...mod, state: 'toggle' }]], [
				['playlist_shuffle', mod, { bgcolor: colors.BLUE, color: colors.WHITE }],
			])
			presets[`${p}_loop`] = button(cat, 'Boucle', 'BOUCLE', [['playlist_loop', { ...mod, state: 'toggle' }]], [
				['playlist_loop', mod, { bgcolor: colors.BLUE, color: colors.WHITE }],
			])
			presets[`${p}_track_info`] = button(
				cat,
				'Morceau en cours',
				`${v(`${p}_track`)}/${v(`${p}_tracks`)}\n${v(`${p}_track_title`)}\n${v(`${p}_track_remaining`)}`,
				[],
				[
					fbPlaying,
					['time_warning', { ...mod, source: 'playlist', seconds: 10 }, { bgcolor: colors.ORANGE, color: colors.WHITE }],
					['progress_bar', bar('playlist', mod)],
				],
			)
		}

		if (m.cue) {
			presets[`${p}_go`] = button(
				cat,
				'GO',
				`GO\n${v(`${p}_cue_standby`)} ${v(`${p}_cue_name`)}`,
				[['cue_go', mod]],
				[
					['cue_playing', mod, { bgcolor: colors.GREEN, color: colors.WHITE }],
					['cue_end', mod, { bgcolor: combineRgb(80, 80, 80), color: colors.WHITE }],
					['progress_bar', bar('cue', mod)],
				],
				combineRgb(0, 90, 0),
			)
			presets[`${p}_cue_pause`] = button(cat, 'Pause / reprise', 'PAUSE', [['cue_pause', { ...mod, state: 'toggle' }]], [
				['cue_paused', mod, { bgcolor: colors.ORANGE, color: colors.WHITE }],
			])
			presets[`${p}_cue_prev`] = button(cat, 'Cue précédent', 'CUE ▲', [['cue_previous', mod]])
			presets[`${p}_cue_next`] = button(cat, 'Cue suivant', 'CUE ▼', [['cue_next', mod]])
			presets[`${p}_cue_info`] = button(
				cat,
				'Cue qui sonne',
				`${v(`${p}_cue_playing_name`)}\n${v(`${p}_cue_remaining`)}`,
				[],
				[
					['cue_playing', mod, { bgcolor: colors.GREEN, color: colors.WHITE }],
					['time_warning', { ...mod, source: 'cue', seconds: 10 }, { bgcolor: colors.ORANGE, color: colors.WHITE }],
					['progress_bar', bar('cue', mod)],
				],
			)
		}

		presets[`${p}_meter`] = button(cat, 'Vumètre', v(`${p}_name`), [], [['module_meter', { ...mod, layout: 'full' }]], colors.BLACK)
		presets[`${p}_stop_meter`] = button(
			cat,
			'Arrêter le module, avec vumètre',
			`STOP\n${v(`${p}_name`)}`,
			[['module_stop', mod]],
			[['module_meter', { ...mod, layout: 'side' }]],
			colors.RED,
		)
	}
	return presets
}

module.exports = { getPresets }
