import { html, css } from "./utils.js";
import { globalStyles } from "./globalStyles.js";

const inputTp = html`<wa-input></wa-input>`;

const inputCss = await css`
  :host {
    width: 100%;
  }
  wa-input::part(base) {
    box-sizing: border-box;
    line-height: 28px;
    padding: 1em;
    margin-top: 1em;
    border-radius: 12px;
    border: 1px solid var(--lightgreen);
    background: var(--extra-light-green);
    font-family: Outfit, sans-serif;
  }
  wa-input::part(base):focus {
    outline: 2px solid var(--green);
  }
  wa-input[invalid]::part(base) {
    outline: 2px solid #ff0000;
  }
  :host([disabled]) > wa-input::part(base) {
    opacity: 0.6;
    cursor: not-allowed;
    background-color: var(--grey-green);
  }
  .error-message {
    color: #ff0000;
    font-size: 0.875em;
    margin-top: 0.25em;
  }
`;

export class InputVirto extends HTMLElement {
  static TAG = "virto-input";
  static formAssociated = true;
  #internals;

  static observedAttributes = [
    "name", "type", "value", "label", "hint", "disabled", "placeholder",
    "readonly", "required", "pattern", "minlength", "maxlength", "min",
    "max", "step", "autocomplete", "autofocus"
  ];

  constructor() {
    super();
    const shadow = this.attachShadow({ mode: "open" });
    shadow.append(inputTp.content.cloneNode(true));
    this.shadowRoot.adoptedStyleSheets = [globalStyles, inputCss];

    this.waInput = shadow.querySelector("wa-input");
    this.errorMessage = shadow.appendChild(document.createElement("div"));
    this.errorMessage.className = "error-message";
    this.#internals = this.attachInternals();
  }

  connectedCallback() {
    this.updateWaInputAttributes();
    this.setupEventForwarding();
    this.waInput.addEventListener("input", this.#handleInput);
    this.waInput.addEventListener("change", this.#handleChange);
    this.waInput.addEventListener("blur", this.validateInput.bind(this));
    if (this.hasAttribute("value")) this.value = this.getAttribute("value");
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue || !this.waInput) return;
    if (name === "disabled") {
      this.waInput.disabled = newValue !== null;
    } else {
      this.waInput.setAttribute(name, newValue || "");
    }
    if (name === "value") this.#syncValue();
  }

  get value() { return this.waInput.value || ""; }
  set value(newValue) {
    this.waInput.value = newValue;
    this.setAttribute("value", newValue);
    this.#syncValue();
    this.validateInput();
  }

  #handleInput = (event) => {
    this.value = event.target.value;
    this.dispatchEvent(new CustomEvent("input", { detail: { value: this.value }, bubbles: true, composed: true }));
  };

  #handleChange = (event) => {
    this.value = event.target.value;
    this.dispatchEvent(new CustomEvent("change", { detail: { value: this.value }, bubbles: true, composed: true }));
  };

  #syncValue() {
    this.#internals.setFormValue(this.value);
  }

  validateInput() {
    const validity = this.waInput.validity;
    let errorMessage = "";

    if (validity.patternMismatch || validity.typeMismatch) {
      errorMessage = this.getAttribute("data-error-message") || "Please enter a valid value.";
    } else if (validity.valueMissing) {
      errorMessage = "This field is required.";
    } else if (validity.tooShort) {
      errorMessage = `Please enter at least ${this.getAttribute("minlength")} characters.`;
    }

    this.waInput.setCustomValidity(errorMessage);
    if (errorMessage) {
      this.waInput.setAttribute("invalid", "");
      this.errorMessage.textContent = errorMessage;
    } else {
      this.waInput.removeAttribute("invalid");
      this.errorMessage.textContent = "";
    }
  }

  updateWaInputAttributes() {
    if (this.waInput) {
      Array.from(this.attributes).forEach((attr) => {
        if (InputVirto.observedAttributes.includes(attr.name)) {
          this.waInput.setAttribute(attr.name, attr.value);
        }
      });
    }
  }

  setupEventForwarding() {
    const events = ["input", "change", "blur", "focus", "invalid"];
    events.forEach((eventName) => {
      this.waInput.addEventListener(eventName, (event) => {
        this.dispatchEvent(new CustomEvent(eventName, { detail: event.detail, bubbles: true, composed: true }));
      });
    });
  }

  get form() { return this.#internals.form; }
  get name() { return this.getAttribute("name"); }
}

if (!customElements.get(InputVirto.TAG)) {
  customElements.define(InputVirto.TAG, InputVirto);
}