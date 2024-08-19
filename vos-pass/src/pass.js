// utilities
const tagFn = fn => (strings, ...parts) => fn(parts
	.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '')
	.concat(strings[parts.length]))
const html = tagFn(s => new DOMParser()
	.parseFromString(`<template>${s}</template>`, 'text/html')
	.querySelector('template'))
const css = tagFn(s => {
	let style = new CSSStyleSheet()
	style.replaceSync(s)
	return style
})

const template = html`
<slot></slot>
<dialog>
  <input id="username" placeholder="foo" />
  <button>submit</button>
</dialog>
`
const style = css`
:host {
  
}
`

/*
 * Pass connects you to the VirtoOS
 */
export class Pass extends HTMLElement { 
  static tag = 'vos-pass'

  #vos = null
  #$modal

  constructor() {
    super()
    let shadow = this.attachShadow({mode: 'closed'})
    shadow.append(template.content.cloneNode(true))
    shadow.adoptedStyleSheets = [style]
    this.#$modal = shadow.querySelector('dialog')
  }

  connectedCallback() {
    console.log('init')
    this.#vos = new Worker('./vos_pass.js', { type: 'module' })
  }

  connect() {
    this.#$modal.openModal()
  }
}
customElements.define(Pass.tag, Pass)
