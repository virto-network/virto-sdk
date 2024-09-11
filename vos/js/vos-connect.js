import './matrix.js'
import { Shell } from './sh.js'
// utilities
const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => new CSSStyleSheet().replace(s))

const formTp = html`
<mx-login-dialog part="login-form"></mx-login-dialog>
`
const formCss = await css`
:host {
}
::slotted(button) {
  width: 100%;
}
`
/*
 * The Connect element includes everything you need to interact with your VOS
 */
export class Connect extends HTMLElement {
  static tag = 'vos-connect'
  static observedAttributes = ['servers']

  // DOM elements
  #$form

  // #onAuth = async (e) => this.connect(e.target.username.value)

  sh = new Shell()

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open', delegatesFocus: true })
    shadow.append(formTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [formCss]

    this.#$form = shadow.querySelector('mx-login-dialog')
  }

  connectedCallback() {
    // this.#$auth.addEventListener('submit', this.#onAuth)
    if (!this.deviceId) this.dataset.deviceId = randString(8)
    console.log(this.getAttribute('servers'))
  }

  attributeChangedCallback(name, a, attr) {
    switch (name) {
      case 'servers': this.#$form.user.serverList = attr.split(' ')
        break
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
