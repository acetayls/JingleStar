// Tests sans Companion : lecture de l'arborescence OSCQuery et encodage OSC.
// Lancer avec « node --test test/ » dans le dossier companion.

const test = require('node:test')
const assert = require('node:assert')
const { parseTree, structureKey, formatTime, parseColor } = require('../src/state')
const { encodeMessage } = require('../src/osc')

const leaf = (v) => ({ VALUE: [v] })

// Extrait de ce que publie JingleStar (src/osc.rs, fonction tree).
const tree = {
	FULL_PATH: '/',
	CONTENTS: {
		jinglestar: {
			CONTENTS: {
				stop_all: { TYPE: 'N' },
				go: { TYPE: 'N' },
				module: {
					CONTENTS: {
						1: {
							CONTENTS: {
								name: leaf('Jingles'),
								mode: leaf('jingle'),
								volume: leaf(-3),
								jingle: {
									CONTENTS: {
										1: { CONTENTS: { name: leaf('Intro'), volume: leaf(-6), playing: leaf(true), color: leaf('#ff8000'), remaining: leaf(4.2) } },
										2: { CONTENTS: { name: leaf(''), volume: leaf(0), playing: leaf(false) } },
									},
								},
							},
						},
						2: {
							CONTENTS: {
								name: leaf('Musique'),
								mode: leaf('playlist'),
								volume: leaf(0),
								playlist: { CONTENTS: { track: leaf(2), tracks: leaf(5), playing: leaf(true), paused: leaf(false), shuffle: leaf(true), loop: leaf(false) } },
							},
						},
						3: {
							CONTENTS: {
								name: leaf('Conduite'),
								mode: leaf('go'),
								volume: leaf(0),
								cue: { CONTENTS: { standby: leaf(0), name: leaf(''), count: leaf(2), playing: leaf(1), remaining: leaf(-1), paused: leaf(true) } },
							},
						},
					},
				},
			},
		},
	},
}

test('lit les modules, jingles, playlist et conduite', () => {
	const s = parseTree(tree)
	assert.strictEqual(s.prefix, '/jinglestar')
	assert.deepStrictEqual(
		s.modules.map((m) => [m.number, m.name, m.mode]),
		[
			[1, 'Jingles', 'jingle'],
			[2, 'Musique', 'playlist'],
			[3, 'Conduite', 'go'],
		],
	)
	const j = s.modules[0].jingles
	assert.strictEqual(j.length, 12)
	assert.deepStrictEqual([j[0].name, j[0].playing, j[0].color, j[0].remaining], ['Intro', true, [255, 128, 0], 4.2])
	assert.strictEqual(j[1].loaded, false)
	assert.strictEqual(j[1].color, null, 'ancienne version : pas de couleur')
	assert.deepStrictEqual([s.modules[1].playlist.track, s.modules[1].playlist.shuffle, s.modules[1].playlist.remaining], [2, true, null])
	assert.strictEqual(s.modules[2].cue.standby, null, '0 : fin de la cuelist')
	assert.strictEqual(s.modules[2].cue.remaining, null, '-1 : pas de temps restant')
	assert.strictEqual(s.modules[2].cue.paused, true)
	assert.strictEqual(structureKey(s), '/jinglestar|1:jingle,2:playlist,3:go')
})

test('préfixe vide', () => {
	const s = parseTree({ CONTENTS: { stop_all: {}, module: { CONTENTS: {} } } })
	assert.strictEqual(s.prefix, '')
	assert.strictEqual(parseTree({ CONTENTS: { autre: {} } }), null)
})

test('formats', () => {
	assert.strictEqual(formatTime(65.2), '1:06')
	assert.strictEqual(formatTime(null), '')
	assert.deepStrictEqual(parseColor([10, 20, 30]), [10, 20, 30])
	assert.strictEqual(parseColor('rouge'), null)
})

test('encode un message OSC', () => {
	const b = encodeMessage('/jinglestar/module/1/volume', [{ type: 'f', value: -6 }])
	// Adresse (28 octets avec bourrage), « ,f » (4 octets), réel (4 octets).
	assert.strictEqual(b.length, 36)
	assert.strictEqual(b.toString('utf8', 28, 30), ',f')
	assert.strictEqual(b.readFloatBE(32), -6)
	const t = encodeMessage('/x', [true])
	assert.strictEqual(t.toString('utf8', 4, 6), ',T')
	assert.strictEqual(t.length, 8)
})

test('décode un paquet groupé de niveaux', () => {
	const { decodePacket } = require('../src/osc')
	const m1 = encodeMessage('/jinglestar/module/1/meter', [-6, -12])
	const m2 = encodeMessage('/jinglestar/module/2/meter', [-120, -120])
	const size = (b) => {
		const s = Buffer.alloc(4)
		s.writeInt32BE(b.length)
		return s
	}
	const head = Buffer.concat([Buffer.from('#bundle\0'), Buffer.alloc(8)])
	const bundle = Buffer.concat([head, size(m1), m1, size(m2), m2])
	const out = decodePacket(bundle)
	assert.deepStrictEqual(out[0], { address: '/jinglestar/module/1/meter', args: [-6, -12] })
	assert.strictEqual(out[1].address, '/jinglestar/module/2/meter')
	assert.deepStrictEqual(decodePacket(Buffer.from('n')), [])
})

test('dessins', () => {
	const { progressBar, stereoMeter, meterFraction } = require('../src/graphics')
	const bar = progressBar(10, 2, 0.5, 0xff0000)
	assert.strictEqual(bar.length, 10 * 2 * 4)
	assert.deepStrictEqual([...bar.subarray(0, 4)], [255, 0, 0, 255], 'début rempli')
	assert.strictEqual(bar[9 * 4 + 3], 160, 'fin vide')
	assert.strictEqual(meterFraction(-120), 0)
	assert.strictEqual(meterFraction(0), 1)
	const meter = stereoMeter(20, 60, [0, -120])
	const px = (x, y) => [...meter.subarray((y * 20 + x) * 4, (y * 20 + x) * 4 + 4)]
	assert.deepStrictEqual(px(4, 0), [230, 30, 30, 255], 'gauche au maximum : rouge en haut')
	assert.deepStrictEqual(px(4, 59), [30, 200, 60, 255], 'vert en bas')
	assert.deepStrictEqual(px(14, 59), [0, 0, 0, 170], 'droite muette')
})
