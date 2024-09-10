// utilities
const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => new CSSStyleSheet().replace(s))

const userIdTp = html`
<div id="userId">
  <span>@</span><input id="user" />
  <span>:</span><input id="server" list="server-list" />
  <datalist id="server-list"></datalist>
</div>
`
const userIdCss = await css`
:host {
  --c-accent: var(--color-accent, #888);
  --c-outline: var(--color-outline, rgba(0, 0, 0, 0.4));
  --input-radius: 2px;
  --field-user-width: 3ch;
  --field-server-width: 15ch;
  border-radius: var(--input-radius);
  border: 1px solid var(--c-outline);
  box-sizing: border-box;
  display: block;
  margin: 0.5rem 0;
  padding: 0.5rem 0.3rem;
  min-width: 20ch;
}
:host(:focus) { border: 1px solid var(--c-accent); }
:host(.inline) { display: inline; }
#userId {
  display: inline-flex;
}
span, input { font-family: monospace; }
span { margin: 0 1px; color: var(--c-outline); }
input {
  border: none;
  padding: 0;
  outline: none;
  font-size: 1.1em;
}
#user { width: var(--field-user-width); }
#server { width: var(--field-server-width); opacity: 0.9; }
`

export class UserId extends HTMLElement {
  static tag = 'mx-id'
  static defaultServers = ['matrix.org']

  #$list
  #$server
  #$user

  #serverList = []

  #adjustUserInput = () => {
    this.#$user.value = this.#$user.value.trim()
    if (this.#$user.value.startsWith('@')) {
      this.#$user.value = this.#$user.value.slice(1)
    }
    if (this.#$user.value.endsWith(':')) {
      this.#$user.value = this.#$user.value.slice(0, -1)
      this.#$server.focus()
    }
    const min = getComputedStyle(this).getPropertyValue('--field-user-width').slice(0, -2)
    this.#$user.style.width = `${Math.max(min, this.#$user.value.length)}ch`
  }
  #adjustServerInput = () => {
    const min = getComputedStyle(this).getPropertyValue('--field-server-width').slice(0, -2)
    this.#$server.style.width = `${Math.max(min, this.#$server.value.length)}ch`
  }

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open', delegatesFocus: true })
    shadow.append(userIdTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [userIdCss]

    this.#$user = shadow.querySelector('#user')
    this.#$server = shadow.querySelector('#server')
    this.#$list = shadow.querySelector('#server-list')
    this.serverList = UserId.defaultServers
  }

  connectedCallback() {
    this.#$user.addEventListener('input', this.#adjustUserInput)
    this.#$server.addEventListener('input', this.#adjustServerInput)
  }

  get serverList() { return this.#serverList }
  set serverList(list) {
    if (list.length == 0) return
    this.#serverList = list
    this.#$server.placeholder = list[0]
    list = list.map(server => {
      let opt = document.createElement('option')
      opt.value = server
      return opt
    })
    this.#$list.replaceChildren(...list)
  }

  get mxId() {
    let server = this.#$server.value || this.#serverList[0]
    return parseMxId(`@${this.#$user.value}:${server}`)
  }
}
customElements.define(UserId.tag, UserId)

function parseMxId(id) {
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
