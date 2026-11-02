// Module Bitfocus Companion pour JingleStar.
// Commandes envoyées en OSC (UDP) ; état lu sur le serveur OSCQuery (HTTP) de JingleStar.

const dgram = require('node:dgram')
const { InstanceBase, InstanceStatus, Regex, runEntrypoint } = require('@companion-module/base')

const { encodeMessage } = require('./osc')
const { parseTree, structureKey } = require('./state')
const { getActions } = require('./actions')
const { getFeedbacks } = require('./feedbacks')
const { getVariableDefinitions, getVariableValues } = require('./variables')
const { getPresets } = require('./presets')

class JingleStarInstance extends InstanceBase {
	constructor(internal) {
		super(internal)
		this.state = null
		this.structure = null
		this.oscPort = null
		this.socket = null
		this.timer = null
		this.polling = false
		this.lastJson = ''
		this.lastValues = {}
	}

	async init(config) {
		this.config = config
		this.socket = dgram.createSocket('udp4')
		this.socket.on('error', (e) => this.log('error', `UDP : ${e.message}`))
		this.defineAll()
		this.start()
	}

	async destroy() {
		this.stop()
		try {
			this.socket?.close()
		} catch {
			// déjà fermé
		}
		this.socket = null
	}

	async configUpdated(config) {
		this.config = config
		this.stop()
		this.state = null
		this.oscPort = null
		this.lastJson = ''
		this.defineAll()
		this.start()
	}

	getConfigFields() {
		return [
			{
				type: 'static-text',
				id: 'info',
				width: 12,
				label: 'JingleStar',
				value:
					'Dans JingleStar, ouvrir Préférences → Pilotage à distance et activer l’OSC et OSCQuery. ' +
					'Le port OSC et le préfixe sont lus automatiquement.',
			},
			{ type: 'textinput', id: 'host', label: 'Adresse IP de l’ordinateur JingleStar', width: 8, default: '127.0.0.1', regex: Regex.HOSTNAME },
			{ type: 'number', id: 'query_port', label: 'Port OSCQuery (HTTP)', width: 4, default: 8001, min: 1, max: 65535 },
			{ type: 'number', id: 'osc_port', label: 'Port OSC (UDP), si OSCQuery ne répond pas', width: 4, default: 8000, min: 1, max: 65535 },
			{ type: 'number', id: 'poll', label: 'Rafraîchissement de l’état (ms)', width: 4, default: 250, min: 100, max: 5000 },
		]
	}

	// ------------------------------------------------------------ état

	module(n) {
		return this.state?.modules.find((m) => m.number === n)
	}

	defineAll() {
		this.setActionDefinitions(getActions(this))
		this.setFeedbackDefinitions(getFeedbacks(this))
		this.setVariableDefinitions(getVariableDefinitions(this.state))
		this.setPresetDefinitions(getPresets(this.state, this.label))
		this.lastValues = {}
		this.publishValues()
	}

	publishValues() {
		const values = getVariableValues(this.state)
		const changed = {}
		for (const [k, v] of Object.entries(values)) {
			if (this.lastValues[k] !== v) changed[k] = v
		}
		this.lastValues = values
		if (Object.keys(changed).length) this.setVariableValues(changed)
	}

	start() {
		this.updateStatus(InstanceStatus.Connecting)
		const every = Math.max(100, Number(this.config.poll) || 250)
		this.timer = setInterval(() => this.poll(), every)
		this.poll()
	}

	stop() {
		if (this.timer) clearInterval(this.timer)
		this.timer = null
	}

	base() {
		return `http://${this.config.host}:${this.config.query_port}`
	}

	async getJson(path) {
		const res = await fetch(this.base() + path, { signal: AbortSignal.timeout(1500) })
		if (!res.ok) throw new Error(`HTTP ${res.status}`)
		return res.json()
	}

	async poll() {
		if (this.polling) return
		this.polling = true
		try {
			if (this.oscPort === null) {
				const info = await this.getJson('/?HOST_INFO')
				this.oscPort = Number(info.OSC_PORT) || Number(this.config.osc_port)
			}
			const tree = await this.getJson('/')
			const json = JSON.stringify(tree)
			if (json === this.lastJson) return
			this.lastJson = json
			const state = parseTree(tree)
			if (!state) throw new Error('réponse inattendue (est-ce bien JingleStar ?)')
			this.onState(state)
			this.updateStatus(InstanceStatus.Ok)
		} catch (e) {
			if (this.state !== null || this.lastJson !== 'error') {
				this.log('warn', `JingleStar injoignable : ${e.message}`)
				this.lastJson = 'error'
				this.oscPort = null
				this.onState(null)
			}
			this.updateStatus(InstanceStatus.ConnectionFailure, e.message)
		} finally {
			this.polling = false
		}
	}

	onState(state) {
		const key = structureKey(state)
		this.state = state
		if (key !== this.structure) {
			// Modules ajoutés ou changés de type : nouvelles listes de choix, variables et presets.
			this.structure = key
			this.defineAll()
		} else {
			this.publishValues()
		}
		this.checkFeedbacks()
	}

	// ------------------------------------------------------------ envoi

	send(path, args = []) {
		const port = this.oscPort ?? Number(this.config.osc_port)
		const address = (this.state?.prefix ?? '/jinglestar') + path
		try {
			this.socket?.send(encodeMessage(address, args), port, this.config.host)
		} catch (e) {
			this.log('error', `OSC : ${e.message}`)
		}
	}
}

runEntrypoint(JingleStarInstance, [])
