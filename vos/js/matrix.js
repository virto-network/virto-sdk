// utilities
const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => new CSSStyleSheet().replace(s))
const LANG = navigator.language.split('-')[0]
const i18 = new Proxy({
  en: { user: 'user', pwd: 'password', connect: 'connect', mIdError: 'Invalid user Id', prompt: 'Type here ...' },
  es: { user: 'usuario', pwd: 'contraseña', connect: 'conectar', mIdError: 'Id de usuario invalido', prompt: 'Escribe aquí ...' },
}, { get: (i18, prop) => i18[LANG in i18 ? LANG : 'en'][prop] })

/*
 * MxId is an input field to enter valid Matrix IDs
 */
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
  padding: 0.5rem 1ch;
}
::placeholder {
  font-size: 0.8rem;
  color: rgba(0, 0, 0, 0.4)
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
  overflow: hidden;
  width: 100%;
}
span, input { font-family: monospace; }
span { align-self: center; margin: 0 1px; color: var(--c-outline); }
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
  static TAG = 'mx-id'
  static defaultServers = ['matrix.org']
  static formAssociated = true

  #$list
  #$server
  #$user

  #internals
  #serverList = []
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
    this.serverList = [...MxId.defaultServers]
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
customElements.define(MxId.TAG, MxId)

export function parseMxId(id) {
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

/*
 *
 */
const formTp = html`
<div id="context"><slot></slot></div>
<form>
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
`
const formCss = await css`
:host {
  --mx-login-width: 18rem;
  --mx-login-height: 20rem;
  box-sizing: border-box;
  color: var(--colot-txt, #111);
  container-type: size;
  display: flex;
  flex-direction: column;
  font-familty: sans;
  height: var(--mx-login-height);
  justify-content: center;
  overflow-y: auto;
  padding: 1rem;
  width: var(--mx-login-width);
}
::slotted(p) {
  margin: 0;
}
#context {
  color: var(--color-txt, #111);
  font-family: sans;
  margin-bottom: 0.5rem;
  width: 100%;
}
form {
  box-sizing: border-box;
  width: 100%;
}
#pwd, #id { margin: 0.2rem 0 0.4rem; width: 100%; }
#pwd { font-family: mono; min-width: 8ch; }
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
  font-size: 1em;
}
@container (height < 210px) {
  .label { display: none !important; }
}
@container (width > 500px) and (height < 150px) {
  .label { display: inline !important; }
}
@container (height < 150px) {
  #context { font-size: 0.9em; }
  button { margin: 0 0.3rem; width: fit-content; }
  form { display: flex; }
  label .label { display: inline; margin: auto 0.3rem auto 0; }
  label { display: flex; height: 100%; font-size: 0.9em; margin-right: 0.3rem }
  :is(#id, #pwd.input) { margin: 0; }
}
`
export class LoginForm extends HTMLElement {
  static TAG = 'mx-login-form'

  #$form

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open', delegatesFocus: true })
    shadow.append(formTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [inputCss, formCss]

    this.#$form = shadow.querySelector('form')
    this.user = shadow.querySelector('mx-id')
  }

  connectedCallback() {
    this.#$form.addEventListener('submit', this.#onSubmit)
  }

  #onSubmit = (e) => {
    e.preventDefault()
    console.log(new FormData(this.#$form))
  }
}
customElements.define(LoginForm.TAG, LoginForm)

/*
 *
 */
const promptTp = html`
<header part="header"><slot name="header"></slot></header>
<main class="empty">
  <span id="placeholder">${i18.prompt}</span>
  <pre id="input" contenteditable><br></pre>
</main>
<aside part="extra"><slot name="extra"></slot></aside>
`
const promptCss = await css`
:host { display: inline-flex; width: 50vw; }
:host([type=code]) #input { font-family: monospace; }
:host(:focus) {
  & main { border: 1px solid var(--color-accent); }
  & #placeholder { display: none; }
}
main {
  border: 1px solid var(--color-outline);
  box-sizing: border-box;
  height: 100%;
  padding: 0.3rem;
  position: relative;
  width: 100%;
  &:not(.empty) #placeholder { display: none; }
}
#placeholder {
  position: absolute;
  pointer-events: none;
  top: 0.3rem; left: 0.3rem;
  color: rgba(0, 0, 0, 0.4);
  font-size: 0.9em;
}
#input {
  font-family: sans-serif;
  margin: 0;
  line-height: 1.2em;
  outline: none;
  width: 100%;
  overflow: hidden;
  overflow-wrap: break-word;
  white-space: pre-wrap;
}
`
export class Prompt extends HTMLElement {
  static TAG = 'mx-prompt'

  #$input

  #lastMessage
  #lastMsgReady

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'closed', delegatesFocus: true })
    shadow.append(promptTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [promptCss]

    this.#$input = shadow.querySelector('#input')
    this.#lastMessage = new Promise(res => this.#lastMsgReady = res)
  }

  connectedCallback() {
    this.addEventListener('blur', () => this.#$input.parentElement.classList.toggle('empty', !this.value))
    this.#$input.addEventListener('keypress', this.#enterSubmitOrNewLine)
  }

  send() {
    if (!this.value) return;
    this.#lastMsgReady(this.value)
    this.#$input.innerHTML = '\n'
    this.#lastMessage = new Promise(res => this.#lastMsgReady = res)
  }

  get value() { return this.#$input.textContent.trim() }

  async*[Symbol.asyncIterator]() {
    while (this.#lastMessage) yield this.#lastMessage
  }

  #enterSubmitOrNewLine = e => {
    if (e.code == 'Enter') {
      e.preventDefault();
      if (e.shiftKey) {
        this.#$input.append('\n')
        getSelection().collapse(this.#$input.lastChild);
      } else {
        this.send()
      }
    }
  }
}
customElements.define(Prompt.TAG, Prompt)

/*
 *
 */
const msgTp = html`
<time part="time"></time>
<div id="content"></div>
`
const msgCss = await css`
:host {
  --msg-spacing: 0.5rem;
  display: block;
  padding: var(--msg-spacing);
  position: relative;
}
#content p {
  margin: var(--msg-spacing) 0;
  overflow-wrap: break-word;
  font-family: sans;
}
time {
  color: rgba(0, 0, 0, 0.3);
  font-style: italic;
  font-size: 0.7em;
  position: absolute;
  bottom: var(--msg-spacing);
  right: var(--msg-spacing);
}
`
export class Message extends HTMLElement {
  static TAG = 'mx-msg'

  #$time
  #$content
  #time = null
  #msg = ''

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'closed' })
    shadow.append(msgTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [msgCss]

    this.#$time = shadow.querySelector('time')
    this.#$content = shadow.querySelector('#content')
  }

  connectedCallback() {
    this.time = this.getAttribute('time') ?? ''
  }

  get message() { return this.#msg }
  set message(msg) {
    this.#msg = msg.trim().replace(/\n{2,}/g, '\n\n')
    this.append(this.#msg)
    this.#$content.append(...this.#msg.split('\n\n')
      .map(txt => {
        txt = txt.split('\n')
        txt = txt.reduce((lines, l, i) => {
          lines.push(l)
          if (i < txt.length - 1) lines.push(document.createElement('br'))
          return lines
        }, [])
        let p = document.createElement('p')
        p.append(...txt)
        return p
      })
    )
  }

  get time() { return this.#time }
  set time(date) {
    date = new Date(date)
    if (isNaN(date)) date = new Date()
    this.#time = date
    this.#$time.dateTime = this.#time.toISOString()
    this.#$time.title = ((Date.now() - this.#time.getTime()) < 24 * 3600 * 1000)
      ? this.#time.toLocaleTimeString() : this.#time.toLocaleString()
    this.#updateTime()
  }

  #updateTime = () => {
    const elapsed = (Date.now() - this.#time.getTime()) / 1000
    requestAnimationFrame(() => {
      this.#$time.textContent = this.#formatElapsed(-elapsed)
      if (elapsed < 60) setTimeout(this.#updateTime, 1000 * 10)
      else if (elapsed < 3600) setTimeout(this.#updateTime, 1000 * 60)
    })
  }

  #formatElapsed(elapsed) {
    const f = new Intl.RelativeTimeFormat(LANG, { numeric: 'auto' })
    const ranges = {
      years: 3600 * 24 * 365,
      months: 3600 * 24 * 30,
      weeks: 3600 * 24 * 7,
      days: 3600 * 24,
      hours: 3600,
      minutes: 60,
      seconds: 1
    }
    for (let key in ranges) {
      if (ranges[key] < Math.abs(elapsed)) {
        const delta = elapsed / ranges[key];
        return f.format(Math.round(delta), key);
      }
    }
    return f.format(-0, 'second')
  }
}
customElements.define(Message.TAG, Message)

