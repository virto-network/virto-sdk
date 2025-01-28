

const tagFn = fn => (strings, ...parts) => fn(parts.reduce((tpl, value, i) => `${tpl}${strings[i]}${value}`, '').concat(strings[parts.length]));
const html = tagFn(s => new DOMParser().parseFromString(`<template>${s}</template>`, 'text/html').querySelector('template'));
const css = tagFn(s => s);

const dialogTp = html`
<main>
    <slot></slot>
</main>
`;

const dialogCss = css`
:host {
    display: flex;
    height: 90dvh;
    width: 90vw;
    align-items: center;
    border: 3px solid red;
}
`;

export class Login extends HTMLElement {
    static TAG = 'virto-login';
 
    constructor() {
        super();
        this.attachShadow({ mode: "open" });
        this.shadowRoot.appendChild(dialogTp.content.cloneNode(true));

        const style = document.createElement('style');
        style.textContent = dialogCss;
        this.shadowRoot.appendChild(style);
    }
}

customElements.define(Login.TAG, Login);
