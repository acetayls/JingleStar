// Encodage minimal d'un message OSC 1.0 (entiers, réels, chaînes, booléens).

function pad(buf) {
	const rest = buf.length % 4
	return rest === 0 ? buf : Buffer.concat([buf, Buffer.alloc(4 - rest)])
}

function oscString(s) {
	// Une chaîne OSC se termine toujours par au moins un octet nul.
	return pad(Buffer.concat([Buffer.from(String(s), 'utf8'), Buffer.alloc(1)]))
}

/**
 * Argument : { type: 'i' | 'f' | 's' | 'T' | 'F', value }.
 * Un nombre ou une chaîne seuls sont acceptés aussi (réel, chaîne).
 */
function normalize(arg) {
	if (typeof arg === 'boolean') return { type: arg ? 'T' : 'F' }
	if (typeof arg === 'number') return { type: 'f', value: arg }
	if (typeof arg === 'string') return { type: 's', value: arg }
	return arg
}

function encodeMessage(address, args = []) {
	const list = args.map(normalize)
	const parts = [oscString(address), oscString(',' + list.map((a) => a.type).join(''))]
	for (const a of list) {
		if (a.type === 'i') {
			const b = Buffer.alloc(4)
			b.writeInt32BE(Math.trunc(a.value))
			parts.push(b)
		} else if (a.type === 'f') {
			const b = Buffer.alloc(4)
			b.writeFloatBE(a.value)
			parts.push(b)
		} else if (a.type === 's') {
			parts.push(oscString(a.value))
		}
	}
	return Buffer.concat(parts)
}

/** Lit une chaîne OSC à `pos` : [texte, position suivante]. */
function readString(buf, pos) {
	let end = buf.indexOf(0, pos)
	if (end < 0) end = buf.length
	return [buf.toString('utf8', pos, end), pos + Math.ceil((end - pos + 1) / 4) * 4]
}

/** Décode un paquet (message ou bundle) en liste de { address, args }. Un paquet abîmé donne []. */
function decodePacket(buf, out = []) {
	try {
		if (buf.toString('utf8', 0, 8) === '#bundle\0') {
			let pos = 16
			while (pos + 4 <= buf.length) {
				const size = buf.readInt32BE(pos)
				decodePacket(buf.subarray(pos + 4, pos + 4 + size), out)
				pos += 4 + size
			}
			return out
		}
		let [address, pos] = readString(buf, 0)
		let tags
		;[tags, pos] = readString(buf, pos)
		if (!address.startsWith('/') || !tags.startsWith(',')) return out
		const args = []
		for (const t of tags.slice(1)) {
			if (t === 'f') {
				args.push(buf.readFloatBE(pos))
				pos += 4
			} else if (t === 'i') {
				args.push(buf.readInt32BE(pos))
				pos += 4
			} else if (t === 's') {
				let s
				;[s, pos] = readString(buf, pos)
				args.push(s)
			} else if (t === 'T' || t === 'F') {
				args.push(t === 'T')
			} else {
				break
			}
		}
		out.push({ address, args })
	} catch {
		// paquet tronqué : ignoré
	}
	return out
}

module.exports = { encodeMessage, decodePacket }
