import { MxConnect } from './matrix.js'
import { Shell } from './sh.js'
// utilities
// const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
// const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
// const css = tagFn(s => new CSSStyleSheet().replace(s))
const SRC_URL = new URL(import.meta.url) 
const PARAMS = SRC_URL.searchParams

/*
 * The Connect element includes everything you need to interact with VOS
 */
export class Connect extends MxConnect {
  static tag = 'vos-connect'
  static observedAttributes = ['servers']

  sh = new Shell()

  // constructor() {
  //   super()
  // }

  connectedCallback() {
    super.connectedCallback()
    if (!this.deviceId) this.dataset.deviceId = randString(8)
  }

  attributeChangedCallback(name, _a, attr) {
    switch (name) {
      case 'servers': this.user.serverList = attr.split(' ')
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
  await navigator.credentials.create({
    publicKey: {
      challenge: [],
      rp: {
        name: 'VOS',
        id: SRC_URL.host,
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
      attestation: 'none',
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
