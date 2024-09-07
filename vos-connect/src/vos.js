import { Shell } from './sh.js'
// utilities
const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => new CSSStyleSheet().replace(s))

const formTp = html`
<form id="auth" method="dialog">
  <vos-id></vos-id>
  <button>connect with passkey</button>
</form>
`
const formCss = await css`
:host {
}
::slotted(button) {
  width: 100%;
}
`

/*
 * Pass connects you to the VirtoOS
 */
export class Connect extends HTMLElement {
  static tag = 'vos-connect'
  static observedAttributes = ['servers']

  // DOM elements
  #$input
  #$auth
  #$list = document.createElement('datalist')

  #onAuth = async (e) => this.connect(e.target.username.value)

  sh = new Shell()
  serverList = []

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open' })
    shadow.append(formTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [formCss]

    this.#$input = shadow.querySelector('vos-id')
    this.#$auth = shadow.querySelector('form')
  }

  connectedCallback() {
    this.#$auth.addEventListener('submit', this.#onAuth)
    if (!this.deviceId) this.dataset.deviceId = randString(8)
  }

  attributeChangedCallback(_name, _, attr) {
    this.serverList = attr.split(' ')
    if (this.serverList.length > 0) {
      this.#$list.id = 'server-list'
      this.#$list.replaceChildren(...this.serverList
        .map(server => {
          let opt = document.createElement('option')
          opt.value = server
          return opt
        })
      )
      this.#$input.replaceChildren(this.#$list)
    }
  }

  async connect(user) {
    if (!(window.PublicKeyCredential &&
      PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable &&
      PublicKeyCredential.isConditionalMediationAvailable)) {
      await Promise.all([
        PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable(),
        PublicKeyCredential.isConditionalMediationAvailable(),
      ]).then(results => results.every(r => r === true))
    }
    await this.sh.connect(user)
  }

  get deviceId() { return this.dataset.deviceId }
}
customElements.define(Connect.tag, Connect)

const userIdTp = html`
<div id="userId">
  <span id="at">@</span>
  <input id="user" placeholder="_" tabindex="1" />
  <span>:</span>
  <input id="server" placeholder="matrix.org" list="server-list" />
  <slot>
    <datalist id="server-list"><option value="matrix.org"></option></datalist>
  </slot>
</div>
`
const userIdCss = await css`
:host {
  --color-border: #AAA;
  --color-accent: lightgreen;
  --input-radius: 1px;
  border-radius: var(--input-radius);
  border: 1px solid var(--color-border);
  display: block;
  margin: 0.5rem 0;
  padding: 0.5rem 0.3rem;
}
:host(:focus) { border: 1px solid var(--color-accent); }
:host(.inline) { display: inline; }
#userId {
  display: inline-flex;
}
span, input { font-family: monospace; }
span { margin: 0 1px; }
#at { visibility: hidden; color: rgba(0, 0, 0, 0.5) }
input {
  border: none;
  padding: 0;
  outline: none;
  font-size: 1.1em;
}
#user { width: 1ch; }
#server { width: 10ch; opacity: 0.9; }
`

export class UserId extends HTMLElement {
  static tag = 'vos-id'
  #$user
  #$server
  #$at

  #adjustUserInput = ({target: input}) => {
    this.#$at.style.visibility = 'visible'
    if (input.value.startsWith('@')) {
      input.value = input.value.slice(1)
    }
    if (input.value.endsWith(':')) {
      input.value = input.value.slice(0, -1)
      this.#$server.focus()
    }
    input.style.width = `${Math.max(1, input.value.length)}ch`
  }
  #adjustServerInput = ({target: input}) => {
    input.style.width = `${Math.max(10, input.value.length)}ch`
  }

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open', delegatesFocus: true })
    shadow.append(userIdTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [userIdCss]

    this.#$at = shadow.querySelector('#at')
    this.#$user = shadow.querySelector('#user')
    this.#$server = shadow.querySelector('#server')
  }

  connectedCallback() {
    this.#$user.addEventListener('input', this.#adjustUserInput)
    this.#$server.addEventListener('input', this.#adjustServerInput)
  }
}
customElements.define(UserId.tag, UserId)

async function createAuthCredential(mxid, challenge) {
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
