import { html, css } from './utils.js';
import { globalStyles } from './globalStyles.js';

const dropdownTp = html`
  <label for="dropdown-button"></label>
  <wa-dropdown>
    <virto-button id="dropdown-button" slot="trigger" caret aria-haspopup="listbox" tabindex="0"></virto-button>
    <wa-menu role="listbox">
    </wa-menu>
  </wa-dropdown>
`;

const dropdownCss = await css`
  :host {
    display: inline-block;
    --dropdown-background: var(--darkseagreen);
    --dropdown-text: var(--darkslategray);
    --dropdown-hover: var(--whitish-green);
    --dropdown-focus: var(--lightgreen);
    font-family: var(--font-primary);
    width: 100%;
  }

  label {
    display: block;
    font-size: 18px;
    font-weight: 400;
    color: var(--dropdown-text);
    margin-bottom: 8px;
  }

  wa-dropdown {
    background: var(--dropdown-background);
    border-radius: 36px;
  }

  wa-menu {
    background: var(--dropdown-background);
    border-radius: 12px;
    padding: 8px 0;
    width: 140%;
    max-height: 300px;
    overflow-y: auto;
  }

  wa-menu-item {
    padding: 8px 16px;
    cursor: pointer;
    transition: background-color 0.2s ease-in;
    color: var(--dropdown-text);
  }

  wa-menu-item:hover,
  wa-menu-item:focus {
    background-color: var(--dropdown-hover);
  }
`;

export class DropdownVirto extends HTMLElement {
  static get TAG() { return "virto-dropdown"; }

  static get observedAttributes() {
    return ['label', 'placeholder', 'items', 'open', 'disabled', 'placement'];
  }

  constructor() {
    super();
    const shadow = this.attachShadow({ mode: 'open' });
    shadow.append(dropdownTp.content.cloneNode(true));
    shadow.adoptedStyleSheets = [globalStyles, dropdownCss];

    this.labelEl = shadow.querySelector('label');
    this.dropdown = shadow.querySelector('wa-dropdown');
    this.button = shadow.querySelector('virto-button');
    this.menu = shadow.querySelector('wa-menu');

    this.labelEl.textContent = this.getAttribute('label') || 'Dropdown';
    this.button.setAttribute('label', this.getAttribute('placeholder') || 'Select an option');
    this.updateMenuItems();

    this.setupEventListeners();
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue || !this.dropdown) return;
    switch (name) {
      case 'label':
        this.labelEl.textContent = newValue || 'Dropdown';
        break;
      case 'placeholder':
        if (!this.hasAttribute('value')) {
          this.button.setAttribute('label', newValue || 'Select an option');
        }
        break;
      case 'items':
        this.updateMenuItems();
        break;
      default:
        const value = this.getAttribute(name);
        if (value !== null) this.dropdown.setAttribute(name, value);
        else this.dropdown.removeAttribute(name);
    }
  }

  setupEventListeners() {
    this.dropdown.addEventListener('wa-select', this.handleSelect.bind(this));
  }

  handleSelect(event) {
    const selectedItem = event.detail.item;
    const value = selectedItem.getAttribute('value');
    this.setAttribute('value', value);
    this.dispatchEvent(new CustomEvent('change', { detail: { value } }));
    this.button.setAttribute('label', value);
  }

  updateMenuItems() {
    const items = JSON.parse(this.getAttribute('items') || '["Option 1", "Option 2", "Option 3"]');
    this.menu.innerHTML = items.map(item => `
      <wa-menu-item role="option" value="${item}">${item}</wa-menu-item>
    `).join('');
  }

  updateTranslations(translate) {
    this.labelEl.textContent = translate('dropdown.label');
    this.button.setAttribute('label', this.getAttribute('value') || translate('dropdown.placeholder'));
    const items = translate('dropdown.items', []);
    this.menu.innerHTML = items.map(item => `
      <wa-menu-item role="option" value="${item}">${item}</wa-menu-item>
    `).join('');
  }
}

if (!customElements.get(DropdownVirto.TAG)) {
  customElements.define(DropdownVirto.TAG, DropdownVirto);
}