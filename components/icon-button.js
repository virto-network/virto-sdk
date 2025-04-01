import { html, css } from './utils.js';
import { globalStyles } from './globalStyles.js';

const iconButtonTp = html`
  <wa-icon-button></wa-icon-button>
  <input type="file" style="display: none;" />
`;

const iconButtonCss = await css`
  :host {
    display: inline-block;
  }

  wa-icon-button::part(base) {
    font-family: var(--font-primary);
    cursor: pointer;
    width: 44px;
    height: 44px;
    min-height: 44px;
    padding: 10px;
    border-radius: 50%;
    border: 1px solid var(--black);
    opacity: 1;
    background-color: var(--green);
    color: var(--whitesmoke);
    transition: background-color 500ms ease, color 500ms ease;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  wa-icon-button::part(base):hover {
    background-color: var(--lightgreen);
    color: var(--darkslategray);
  }

  wa-icon-button::part(base):focus {
    outline: 2px solid var(--darkslategray);
  }

  :host([variant="secondary"]) wa-icon-button::part(base) {
    background-color: var(--extra-light-green);
    color: var(--darkslategray);
    border: 1px solid var(--lightgreen);
  }

  :host([variant="secondary"]) wa-icon-button::part(base):hover,
  :host([variant="secondary"]) wa-icon-button::part(base):focus {
    background-color: var(--whitish-green);
  }

  :host([disabled]) wa-icon-button::part(base) {
    background-color: var(--grey-green);
    color: var(--darkslategray);
    border: none;
    cursor: not-allowed;
    opacity: 0.6;
  }
`;

export class IconButtonVirto extends HTMLElement {
  static get TAG() { return "virto-icon-button"; }

  static get observedAttributes() {
    return ['name', 'label', 'variant', 'disabled', 'href', 'target', 'download'];
  }

  constructor() {
    super();
    const shadow = this.attachShadow({ mode: 'open' });
    shadow.append(iconButtonTp.content.cloneNode(true));
    shadow.adoptedStyleSheets = [globalStyles, iconButtonCss];

    this.waIconButton = shadow.querySelector('wa-icon-button');
    this.fileInput = shadow.querySelector('input[type="file"]');

    this.syncAttributes();
    this.setupEventListeners();
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue || !this.waIconButton) return;
    this.syncAttributes();
  }

  syncAttributes() {
    const attrs = {
      name: this.getAttribute('name') || '',
      label: this.getAttribute('label') || '',
      variant: this.getAttribute('variant') || null,
      disabled: this.hasAttribute('disabled'),
      href: this.getAttribute('href') || null,
      target: this.getAttribute('target') || null,
      download: this.getAttribute('download') || null
    };
    Object.entries(attrs).forEach(([attr, value]) => {
      if (value !== null && value !== false) {
        this.waIconButton.setAttribute(attr, value);
      } else if (attr === 'disabled') {
        if (value) this.waIconButton.setAttribute(attr, '');
        else this.waIconButton.removeAttribute(attr);
      } else {
        this.waIconButton.removeAttribute(attr);
      }
    });
  }

  setupEventListeners() {
    this.waIconButton.addEventListener('click', () => {
      if (this.getAttribute('name') === 'paperclip') {
        this.attachDocument();
      }
    });
    this.fileInput.addEventListener('change', this.handleFileSelection.bind(this));
  }

  attachDocument() {
    this.fileInput.click();
  }

  // TODO: Solve what to do with the file attached (next iteration)
  handleFileSelection(event) {
    const file = event.target.files[0];
    if (file) {
      this.dispatchEvent(new CustomEvent('file-attached', {
        detail: { file },
        bubbles: true,
        composed: true
      }));
    }
    this.fileInput.value = '';
  }
}

if (!customElements.get(IconButtonVirto.TAG)) {
  customElements.define(IconButtonVirto.TAG, IconButtonVirto);
}