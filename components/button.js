import { html, css } from "./utils.js";
import { globalStyles } from "./globalStyles.js";

const buttonTp = html`
  <wa-button></wa-button>
`;

const buttonCss = await css`
  :host {
    display: inline-block;
    width: 100%;
  }
  wa-button::part(base) {
    font-family: Outfit, sans-serif;
    width: 100%;
    height: 44px;
    min-height: 44px;
    padding: 12px;
    border-radius: 1000px;
    border: 1px solid #1A1A1A1F;
    opacity: 1;
    background-color: var(--green);
    color: var(--whitesmoke);
    transition: background-color 500ms ease, color 500ms ease;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  wa-button::part(base):hover {
    background-color: var(--lightgreen);
    color: var(--darkslategray);
  }
  wa-button::part(base):focus {
    outline: 2px solid var(--darkslategray);
  }
  :host([variant="secondary"]) > wa-button::part(base) {
    background-color: var(--extra-light-green);
    color: var(--darkslategray);
    border: 1px solid var(--lightgreen);
  }
  :host([variant="secondary"]) wa-button::part(base):hover,
  :host([variant="secondary"]) > wa-button::part(base):focus {
    background-color: var(--whitish-green);
  }
  :host([variant="secondary"]) > wa-button::part(base):focus {
    outline: 2px solid var(--green);
  }
  :host([disabled]) > wa-button::part(base), :host([disabled]) > wa-button::part(base):hover {
    background-color: var(--grey-green);
    color: var(--darkslategray);
    border: transparent;
  }
   {
`;

export class ButtonVirto extends HTMLElement {
  static TAG = "virto-button";
  static formAssociated = true;
  #internals;

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.shadowRoot.appendChild(buttonTp.content.cloneNode(true));
    this.shadowRoot.adoptedStyleSheets = [globalStyles, buttonCss];

    this.waButton = this.shadowRoot.querySelector("wa-button");
    this.waButton.textContent = this.getAttribute("label") || "Button";
    this.#internals = this.attachInternals();
    this.#syncAttributes();
  }

  connectedCallback() {
    this.waButton.addEventListener("click", this.#handleClick);
  }

  static observedAttributes = ["label", "variant", "disabled", "type", "loading"];

  attributeChangedCallback(name, oldValue, newValue) {
    if (!this.waButton) return;
    if (name === "label") {
      this.waButton.textContent = newValue || "Button";
    } else {
      this.#syncAttributes();
    }
  }

  #syncAttributes() {
    const attrs = ["variant", "disabled", "type", "loading"];
    attrs.forEach(attr => {
      const value = this.getAttribute(attr);
      if (value !== null) this.waButton.setAttribute(attr, value);
      else this.waButton.removeAttribute(attr);
    });
    if (this.getAttribute("type") === "submit") {
      this.#internals.setFormValue(this.getAttribute("value") || "submit");
    }
  }

  #handleClick = () => {
    if (this.getAttribute("type") === "submit" && this.#internals.form && !this.hasAttribute("disabled")) {
      this.#internals.form.requestSubmit();
    }
  };

  get form() { return this.#internals.form; }
  get name() { return this.getAttribute("name"); }
  get type() { return this.getAttribute("type") || "button"; }
  get value() { return this.getAttribute("value") || "submit"; }
  
}

if (!customElements.get(ButtonVirto.TAG)) {
  customElements.define(ButtonVirto.TAG, ButtonVirto);
}