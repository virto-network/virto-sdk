import { html, css } from "./utils.js";
import { globalStyles } from "./globalStyles.js";

const cardTp = html`
  <wa-card>
    <slot name="image" slot="image"></slot>
    <slot name="header" slot="header"></slot>
    <slot></slot>
    <slot name="footer" slot="footer"></slot>
  </wa-card>
`;

const cardCss = await css`
  :host {
    display: inline-block;
  }

  wa-card {
    background-color: var(--extra-light-green);
    --wa-panel-border-radius: 40px;
    --wa-panel-border-width: 1px;
    --wa-panel-border-color: var(--green);
    --wa-panel-padding: 32px;
    --wa-space: 16px;
    padding: var(--wa-space);
    transition: all 0.2s ease-out;
    font-family: var(--font-primary);
    overflow: hidden;
    height: fit-content;
    color: var(--darkslategray);
  }

  wa-card:hover {
    background-color: var(--whitish-green, #d0e6d0);
    box-shadow: 0px 16px 32px 0px rgba(0, 34, 24, 0.2);
  }

  wa-card::part(header) {
    font-weight: bold;
    margin-bottom: var(--wa-space);
  }

  wa-card::part(body) {
    display: flex;
    justify-content: space-between;
    flex-grow: 1;
  }

  wa-card::part(footer) {
    color: var(--green);
  }

  wa-card::part(image) {
    max-height: 150px;
    object-fit: cover;
  }

  :host([size="small"]) wa-card {
    --wa-panel-padding: 24px;
  }

  :host([size="large"]) wa-card {
    --wa-panel-padding: 40px;
    height: 400px;
  }

  :host([size="small"]) wa-card::part(image) {
    max-height: 100px;
  }

  :host([size="large"]) wa-card::part(image) {
    max-height: 200px;
  }

 :host([with-image]) wa-card {
    padding: 0;
  }
`;

export class CardVirto extends HTMLElement {
  static get TAG() { return "virto-card"; }

  static get observedAttributes() {
    return ["size", "with-header", "with-image", "with-footer"];
  }

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.shadowRoot.appendChild(cardTp.content.cloneNode(true));
    this.shadowRoot.adoptedStyleSheets = [globalStyles, cardCss];

    this.waCard = this.shadowRoot.querySelector("wa-card");
    this.updateCard();
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue || !this.waCard) return;
    this.updateCard();
  }

  updateCard() {
    const attrs = ["size", "with-header", "with-image", "with-footer"];
    attrs.forEach(attr => {
      const value = this.getAttribute(attr);
      if (value !== null) this.waCard.setAttribute(attr, value);
      else this.waCard.removeAttribute(attr);
    });
  }
}

if (!customElements.get(CardVirto.TAG)) {
  customElements.define(CardVirto.TAG, CardVirto);
}