import './matrix.js'
import { Shell } from './sh.js'
// utilities
const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => new CSSStyleSheet().replace(s))
const PARAMS = new URL(import.meta.url).searchParams

/*
 * The Connect element includes everything you need to interact with VOS
 */
const formTp = html`
<dialog id="login">
  <mx-login-form part="login-form">
    <slot name="login-form"></slot>
  </mx-login-form>
</dialog>
<div id="connect"><slot></slot></div>
<div id="connected" hidden><slot name="connected"></slot></div>
`
const formCss = await css`
:host {
  display: inline-block;
  height: 1.8rem;
  vertical-aligh: top;
}
#connect { height: 100%; }
#connect ::slotted(button) {
  background: var(--color-accent);
  border-radius: var(--border-radius, 2px);
  border: none;
  color: white;
  display: inline-block;
  height: 100%;
  padding: 0 0.5rem;
}
dialog#login {
  padding: 0;
  border: none;
}
mx-login-form {
  border: 1px solid var(--color-outline, #999);
}
`
export class Connect extends HTMLElement {
  static tag = 'vos-connect'
  static observedAttributes = ['servers']

  // DOM elements
  #$loginDialog
  #$loginForm
  #$connect

  sh = new Shell()

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open', delegatesFocus: true })
    shadow.append(formTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [formCss]

    this.#$loginDialog = shadow.querySelector('#login')
    this.#$loginForm = this.#$loginDialog.firstElementChild
    this.#$connect = shadow.querySelector('#connect')
  }

  connectedCallback() {
    if (!this.deviceId) this.dataset.deviceId = randString(8)
    this.#$connect.addEventListener('click', () => this.#$loginDialog.showModal())
  }

  attributeChangedCallback(name, _a, attr) {
    switch (name) {
      case 'servers': this.#$loginForm.user.serverList = attr.split(' ')
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
if (PARAMS.get('def') != 'no') customElements.define(Connect.tag, Connect)

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
