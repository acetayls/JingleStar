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

module.exports = { encodeMessage }
