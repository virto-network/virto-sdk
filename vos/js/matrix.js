// utilities
const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => new CSSStyleSheet().replace(s))
const LANG = navigator.language.split('-')[0]
const i18 = new Proxy({
  en: { user: 'user', pwd: 'password', connect: 'connect', mIdError: 'Invalid user Id' },
  es: { user: 'usuario', pwd: 'contraseña', connect: 'conectar', mIdError: 'Id de usuario invalido' },
}, { get: (i18, prop) => i18[LANG in i18 ? LANG : 'en'][prop] })

const userIdTp = html`
<div id="userId" class="input">
  <span>@</span><input id="user" placeholder="${i18.user}" autocapitalize="none" />
  <span>:</span><input id="server" list="server-list" />
  <datalist id="server-list"></datalist>
</div>
`
const inputCss = await css`
:host {
  --c-accent: var(--color-accent, #888);
  --c-outline: var(--color-outline, rgba(0, 0, 0, 0.4));
  --in-radius: var(--input-radius, 2px);
  --in-size: var(--input-size, 2.4rem);
}
.input {
  border-radius: var(--in-radius);
  border: 1px solid var(--c-outline);
  box-sizing: border-box;
  display: block;
  font-size: 0.8rem;
  height: var(--in-size);
  outline: none;
  padding: 0.5rem 0.3rem;
}
`
const userIdCss = await css`
:host {
  --field-user-width: 4ch;
  --field-server-width: 16ch;
  display: block;
  min-width: 20ch;
}
:host(:focus) .input { border: 1px solid var(--c-accent); }
#userId {
  display: inline-flex;
  width: 100%;
}
span, input { font-family: monospace; }
span { margin: 0 1px; color: var(--c-outline); }
input {
  border: none;
  padding: 0;
  outline: none;
  font-size: 1.1em;
  overflow: hidden;
}
#user { width: var(--field-user-width); }
#server { width: var(--field-server-width); opacity: 0.9; }
`
export class MxId extends HTMLElement {
  static tag = 'mx-id'
  static defaultServers = ['matrix.org']
  static formAssociated = true

  #$list
  #$server
  #$user

  #internals
  #serverList = MxId.defaultServers
  value = ''

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'closed', delegatesFocus: true })
    shadow.append(userIdTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [inputCss, userIdCss]
    this.#internals = this.attachInternals()

    this.#$user = shadow.querySelector('#user')
    this.#$server = shadow.querySelector('#server')
    this.#$list = shadow.querySelector('#server-list')
  }

  connectedCallback() {
    this.#$user.addEventListener('input', this.#adjustUserInput)
    this.#$server.addEventListener('input', this.#adjustServerInput)
    this.#checkInput()
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

  // form-associated custom element
  get form() { return this.#internals.form }
  get name() { return this.getAttribute('name') }
  get type() { return this.localName }
  get validity() { return this.#internals.validity }
  get validationMessage() { return this.#internals.validationMessage }
  get willValidate() { return this.#internals.willValidate }
  checkValidity() { return this.#internals.checkValidity() }
  reportValidity() { return this.#internals.reportValidity() }

  #checkInput() {
    let id = this.mxId?.toString()
    if (id) this.#internals.setValidity({})
    else this.#internals.setValidity({ customError: true }, i18.mIdError, this.#$user)
    this.#internals.setFormValue(id)
  }

  // event handlers
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
    this.#checkInput()
  }
  #adjustServerInput = () => {
    const min = getComputedStyle(this).getPropertyValue('--field-server-width').slice(0, -2)
    this.#$server.style.width = `${Math.max(min, this.#$server.value.length)}ch`
  }
}
customElements.define(MxId.tag, MxId)

const formTp = html`
<dialog open>
  <form method="dialog">
    <label>
      <div class="label">${i18.user}</div>
      <mx-id id="id" name="id" required></mx-id>
    </label>
    <label>
      <div class="label">${i18.pwd}</div>
      <input id="pwd" name="pwd" class="input" type="password" placeholder="${i18.pwd}" required />
    </label>
    <button>${i18.connect}</button>
  </form>
</dialog>
`
const formCss = await css`
:host {
  --dialog-border: 1px solid var(--c-outline);
  container-type: size;
  display: block;
  overflow: hidden;
}
dialog {
  position: relative;
  width: 100%;
  height: 100%;
  box-sizing: border-box;
  border: var(--dialog-border);
  padding: 1rem;
  display: flex;
}
form { width: 100%; margin: auto; }
#pwd, #id { margin: 0.2rem 0 0.4rem; width: 100%; }
#pwd { font-family: mono; min-width: 5ch; }
#pwd:focus { border: 1px solid var(--c-accent); }
label {
  color: var(--color-outline, #aaa);
  display: block;
  font-family: sans;
  font-size: 0.8em;
  text-transform: capitalize;
}
button {
  background: var(--c-accent);
  border-radius: var(--in-radius);
  border: none;
  box-sizing: border-box;
  color: white;
  height: var(--in-size);
  margin: 0.3rem 0;
  padding: 0 0.6rem;
  width: 100%;
}
@container (height < 210px) {
  .label { display: none !important; }
}
@container (width > 500px) and (height < 150px) {
  .label { display: inline !important; }
}
@container (height < 150px) {
  dialog { padding: 0 0.8rem; }
  button { margin: 0 0.3rem; width: fit-content; }
  form { display: flex; }
  label .label { display: inline; margin: auto 0.3rem auto 0; }
  label { display: flex; height: 100%; font-size: 0.9em; margin: 0 0.3rem }
  :is(#id, #pwd.input) { margin: 0; }
}
`
/*
 *
 */
export class LoginForm extends HTMLElement {
  static tag = 'mx-login-dialog'

  #$form
  #formInternals

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open', delegatesFocus: true })
    shadow.append(formTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [inputCss, formCss]

    this.#$form = shadow.querySelector('form')
    this.user = shadow.querySelector('mx-id')
  }

  connectedCallback() {
    // this.#$form.addEventListener('submit', this.#foo)
  }
}
customElements.define(LoginForm.tag, LoginForm)

function parseMxId(id) {
  if (!id.startsWith('@')) return null
  let [user, server] = id.slice(1).split(':')
  if (user.length == 0) return null
  if (!server) return null
  try { server = new URL(`https://${server}`) } catch { return null }
  return {
    user,
    server,
    toString() { return `@${this.user}:${this.server.host}` }
  }
}
