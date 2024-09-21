const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => new CSSStyleSheet().replace(s))

export async function* streamingFormData(selector) {
  const form = document.querySelector(selector)
  if (!form) return
  let lastMsgReady = null
  let lastMsg = new Promise(res => lastMsgReady = res)
  form.addEventListener('submit', e => {
    e.preventDefault()
    console.log('submitted')
    lastMsgReady(new FormData(form))
  })
  while (lastMsg) yield lastMsg
}


const switchTp = html`
<ul id="options" popover="auto"></ul>
<button popovertarget="options" popovertargetaction="toggle"></button>
<main></main>
`
const switchCss = await css`
:host {
  display: flex;
  align-items: center;
}
main {
  flex: 1;
  &>* { width: 100%; }
}
#options {
  border: 1px solid var(--color-outline);
  border-radius: 2px;
  &:popover-open {
    position: absolute;
    inset: unset;
    bottom: 0;
    left: 0;
  }
}
li {
  &::before { content: attr(data-ic) '  '; }
  &:hover { background: var(--color-outline, #eee); cursor: pointer; }
}
button {
  background: none;
  border-radius: 1rem;
  border: none;
  color: var(--color-outline);
  font-size: 1.2em;
  height: 1.8rem;
  line-height: 1rem;
  vertical-align: middle;
  white-space: nowrap;
  width: 1.8rem;
}
`
export class Switcher extends HTMLElement {
  static TAG = 'input-switcher'
  static formAssociated = true

  #$btn
  #$options
  #$current
  #internals
  #options = {}

  constructor() {
    super()
    let shadow = this.attachShadow({ mode: 'open', delegatesFocus: true })
    shadow.append(switchTp.content.cloneNode(true))
    shadow.adoptedStyleSheets = [switchCss]
    this.#internals = this.attachInternals()

    this.#$btn = shadow.querySelector('button')
    this.#$options = shadow.querySelector('#options')
    this.#$current = shadow.querySelector('main')
  }

  connectedCallback() {
    this.querySelectorAll('template').forEach(tpl => {
      let value = tpl.dataset.value
      if (!value) return
      this.#options[value] = tpl
    })
    Object.entries(this.#options).map(([opt, t]) => {
      let li = document.createElement('li')
      li.textContent = t.dataset.option
      li.dataset.ic = t.dataset.ic
      li.dataset.value = opt
      this.#$options.append(li)
    })
    this.#$options.addEventListener('click', e => {
      this.select(e.target.dataset.value)
      this.#$options.hidePopover()
    })
    this.select(this.getAttribute('default'))
  }

  select(opt) {
    if (!(opt in this.#options)) return
    this.#$btn.textContent = this.#options[opt].dataset.ic
    const selection = this.#options[opt].content.cloneNode(true)
    this.#$current.replaceChildren(selection)
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
}
customElements.define(Switcher.TAG, Switcher)
