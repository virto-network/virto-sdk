// utilities
const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => new CSSStyleSheet().replace(s))
const timeout = (ms) => new Promise((res) => setTimeout(res, ms));
const timeoutErr = (ms, error) => timeout(ms).then(() => { throw new Error(`timeout ${error}`) })

/*
 * Vos acts as the bridge to the API running in the background worker
 */
class Vos {
  static worker = null

  #lastId = 0
  #pending = {}

  #onMsg = ({ data: out }) => {
    console.log(out)
    this.#pending[id]?.(null)
    delete this.#pending[id]
  }

  constructor() {
    if (!Vos.worker) {
      let params = new URL(import.meta.url).searchParams
      Vos.worker = new Worker(new URL(`vos_pass.js?${params}`, import.meta.url), { type: 'module' })
      Vos.worker.addEventListener('message', this.#onMsg)
    }
  }

  async fetch(path) {
    this.run(`get ${path}`)
  }

  async run(cmd) {
    let resolve
    let res = new Promise((r) => resolve = r)
    let id = this.#lastId += 1
    Vos.worker.postMessage({ id, cmd })
    this.#pending[id] = resolve
    return Promise.race([res, timeoutErr(3000, `cmd #${id}`)])
  }
}

const template = html`
<slot></slot>
<dialog>
  <form id="auth" method="dialog">
    <input id="username" placeholder="foo" autofocus />
    <button>submit</button>
  </form>
</dialog>
`
const style = await css`
:host { }
`

/*
 * Pass connects you to the VirtoOS
 */
export class Pass extends HTMLElement {
  static tag = 'vos-pass'

  vos = new Vos()

  // DOM elements
  #$modal
  #$auth

  #onAuth = async (e) => this.connect(e.target.username.value)

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open' })
    shadow.append(template.content.cloneNode(true))
    shadow.adoptedStyleSheets = [style]

    this.#$modal = shadow.querySelector('dialog')
    this.#$auth = shadow.querySelector('form')
  }

  connectedCallback() {
    this.#$auth.addEventListener('submit', this.#onAuth)
    if (!this.deviceId) this.dataset.deviceId = randString(8)
  }

  open() {
    this.#$modal.show()
  }

  async connect(user) {
    await this.vos.run(`auth ${user}`)
  }

  get deviceId() { return this.dataset.deviceId }
}
customElements.define(Pass.tag, Pass)

function mxId(id) {
  if (!id.startsWith('@')) return
  let [user, server] = id.slice(1).split(':')
  if (user.length == 0) return
  if (!server) return
  try { server = new URL(`https://${server}`) } catch { return }
  return {
    user,
    server,
    toString() { return `@${this.user}:${this.server.host}` }
  }
}

async function authCredential(mxid, challenge) {
  const url = new URL(import.meta.url)
  await navigator.credentials.create({
    publicKey: {
      challenge: [],
      rp: {
        name: 'VOS',
        id: url.host,
      },
      user: {
        id: `${mxid}`,
        name: mxid.user,
        displayName: mxid.user,
      },
      pubKeyCredParams: [
        { type: 'public-key', alg: -8 },
        { type: 'public-key', alg: -7 },
      ],
      authenticatorSelection: {
        userVerification: 'required',
        requireResidentKey: true,
        residentKey: 'required',
      },
      timeout: 90000,
      attestation: 'indirect',
    }
  })
}

function randString(length) {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789';
  return Array.from(
    { length },
    () => chars[Math.floor(Math.random() * chars.length)]
  ).join('');
}
