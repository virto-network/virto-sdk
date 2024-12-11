const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]))
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'))
const css = tagFn(s => s)

const dialogTp = html`
  <div class="dialog">
    <header>
      <slot name="logo"></slot>
      <h2 id="dialog-title"><slot name="title"></slot></h2>
    </header>
    <hr>
    <div class="content">
      <slot name="content"></slot>
    </div>
    <div class="navigation">
      <button-virto id="leftButton" variant="secondary"></button-virto>
      <button-virto id="rightButton"></button-virto>
    </div>
  </div>
`

const dialogCss = css`
  :host {
    all: initial;
    display: none;
    position: fixed;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background-color: #0000004D;
    justify-content: center;
    align-items: center;
    opacity: 0;
    transition: opacity 0.3s ease;
  }

  :host(.visible) {
    display: flex;
    opacity: 1;
  }

  .dialog {
    display: flex;
    flex-direction: column;
    gap: 1em;
    font-family: Outfit, sans-serif;
    width: 80%;
    max-width: 528px;
    height: fit-content;
    background: linear-gradient(0deg, rgba(231, 247, 232, 0.5), rgba(231, 247, 232, 0.5)),
                radial-gradient(84.04% 109.28% at 10.3% 12.14%, rgba(86, 201, 96, 0.5) 0%, rgba(198, 235, 199, 0) 98.5%);
    border-radius: 12px;
    box-shadow: 0px 2px var(--Blurblur-3, 3px) -1px rgba(26, 26, 26, 0.08),
                0px 1px var(--Blurblur-0, 0px) 0px rgba(26, 26, 26, 0.08);
    backdrop-filter: blur(32px);
    padding: 1em;
    gap: clamp(4px, 1vw, var(--spacing7, 14px));
    transform: translateX(100%);
    opacity: 0;
    pointer-events: none;
    transition: transform 0.3s ease, opacity 0.2s ease;
  }

  :host(.visible) .dialog {
    transform: translateX(0);
    opacity: 1;
    pointer-events: auto;
    animation: slideInRight 0.5s forwards;
  }

  @keyframes slideInRight {
    from {
      transform: translateX(100%);
      opacity: 0;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }

  @keyframes slideOutLeft {
    from {
      transform: translateX(0);
      opacity: 1;
    }
    50% {
      opacity: 0;
    }
    to {
      transform: translateX(-100%);
      opacity: 0;
    }
  }

  header {
    display: flex;
    gap: 1em;
  }

  header h2 {
    font-size: 1.4em;
    font-weight: 600;
    color: var(--color-txt);
    margin: 0;
  }

  hr {
    border: none;
    border-radius: 1px;
    border-top: 1px solid var(--color-accent);
    margin: 1em 0;
  }

  .navigation {
    box-sizing: border-box;
    display: flex;
    justify-content: space-between;
    gap: 10px;
    width: 100%;
    flex-direction: row;
    color: var(--color-txt);
  }

  .navigation button-virto {
    flex: 1;
  }
`

export class DialogoModal extends HTMLElement {
  static TAG = 'dialog-virto'

  #$dialog
  #$leftButton
  #$rightButton
  #isClosing = false

  constructor() {
    super()
    const shadow = this.attachShadow({ mode: 'open' })
    shadow.append(dialogTp.content.cloneNode(true))
    
    const style = document.createElement('style')
    style.textContent = dialogCss
    shadow.appendChild(style)

    this.#$dialog = shadow.querySelector('.dialog')
    this.#$leftButton = shadow.getElementById('leftButton')
    this.#$rightButton = shadow.getElementById('rightButton')
  }

  connectedCallback() {
    if (this.#$leftButton) {
      this.#$leftButton.addEventListener('click', () => this.#handleButtonClick('left'))
    }
    if (this.#$rightButton) {
      this.#$rightButton.addEventListener('click', () => this.#handleButtonClick('right'))
    }
  }

  #handleButtonClick(button) {
    const event = new CustomEvent('button-click', { 
      detail: { button },
      bubbles: true,
      composed: true
    })
    this.dispatchEvent(event)
  }

  setButtons(leftLabel, rightLabel) {
    if (this.#$leftButton) {
      if (leftLabel) {
        this.#$leftButton.setAttribute('label', leftLabel)
        this.#$leftButton.style.display = ''
      } else {
        this.#$leftButton.style.display = 'none'
      }
    }

    if (this.#$rightButton) {
      if (rightLabel) {
        this.#$rightButton.setAttribute('label', rightLabel)
        this.#$rightButton.style.display = ''
      } else {
        this.#$rightButton.style.display = 'none'
      }
    }
  }

  show() {
    this.#isClosing = false
    this.classList.add('visible')
  }

  hide() {
    if (this.#isClosing) return
    this.#isClosing = true
    this.#$dialog.style.animation = 'slideOutLeft 0.5s forwards'
    this.#$dialog.addEventListener('animationend', () => {
      this.classList.remove('visible')
      this.dispatchEvent(new CustomEvent('dialog-closed'))
      this.#isClosing = false
    }, { once: true })
  }
}

customElements.define(DialogoModal.TAG, DialogoModal)