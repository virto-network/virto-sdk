import 'https://early.webawesome.com/webawesome@3.0.0-alpha.7/dist/components/dialog/dialog.js';

const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]));
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'));
const css = tagFn(s => s);

const dialogTp = html`
    <wa-dialog light-dismiss with-header with-footer>
    <div slot="label">
        <slot name="logo"></slot>
        <slot name="title"></slot>
    </div>
    <hr> 
    <slot></slot>
    <div slot="footer">
        <button-virto data-dialog="close" label="Close"></button-virto>
        <button-virto class="dialog-next" data-dialog="next" label="Next"></button-virto>
    </div>
    </wa-dialog>
`;

const dialogCss = css`
:host, wa-dialog { font-family: 'Outfit', sans-serif !important; }

wa-dialog::part(base) {
    font-family: 'Outfit', sans-serif;
    font-weight: 400;
    padding: 1em;
    background: var(--color-dialog-bg);
    border-radius: 12px;
    box-shadow: 0px 2px var(--Blurblur-3, 3px) -1px rgba(26, 26, 26, 0.08),
             0px 1px var(--Blurblur-0, 0px) 0px rgba(26, 26, 26, 0.08);
}

wa-dialog::part(header) {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

wa-dialog::part(header-actions) {
    order: 1;
}

wa-dialog::part(title) {
    display: flex;
    align-items: center;
    gap: 1em;
}

wa-dialog::part(footer) {
    display: flex;
    width: 100%;
    justify-content: space-between;
}

hr  { 
    border-top: 1px solid var(--color-accent); 
}

[slot="label"] {
    display: flex;
    align-items: center;
    gap: 1em;
}

[slot="footer"] {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 1em;
}
`;

export class DialogoModal extends HTMLElement {
    static TAG = 'dialog-virto';
 
    constructor() {
        super();
        this.attachShadow({ mode: "open" });
        this.shadowRoot.appendChild(dialogTp.content.cloneNode(true));

        const style = document.createElement('style');
        style.textContent = dialogCss;
        this.shadowRoot.appendChild(style);
    }

    connectedCallback() {
        this.dialog = this.shadowRoot.querySelector('wa-dialog');
        this.nextButton = this.shadowRoot.querySelector("[data-dialog='next']");
        this.closeButton = this.shadowRoot.querySelector("[data-dialog='close']");

        this.nextButton.addEventListener("click", () => this.next());
        this.closeButton.addEventListener("click", () => this.close());

        customElements.whenDefined('wa-dialog').then(() => {
            console.log('wa-dialog is defined');
        });
    }

    async open() {
        await this.dialog.updateComplete;
        this.dialog.open = true;
    }

    async close() {
        await this.dialog.updateComplete;
        this.dialog.open = false;
    }

    async next() {
        const allDialogs = document.querySelectorAll("dialog-virto");
        const currentIndex = Array.from(allDialogs).indexOf(this);
        if (currentIndex + 1 < allDialogs.length) {
            await this.close();
            await allDialogs[currentIndex + 1].open();
        }
    }
}

customElements.define(DialogoModal.TAG, DialogoModal);
