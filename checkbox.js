class CustomCheckbox extends HTMLElement {
    static TAG = 'checkbox-virto';
    static css = `
    :host {
        display: block;
        width: 100%;
    }
    .checkbox-container {
        display: flex;
        align-items: center;
        justify-content: center;
        cursor: pointer;
    }
    input[type="checkbox"] {
        appearance: none;
        width: 20px;
        height: 20px;
        border-radius: 4px;
        background-color: var(--checkbox-bg, #e0e0e0);
        border: 1px solid var(--checkbox-border, #ccc);
        display: grid;
        place-content: center;
        margin-right: 10px;
    }
    input[type="checkbox"]:checked {
        background-color: var(--checkbox-checked-bg, #24AF37);
        border-color: var(--checkbox-checked-border, #24AF37);
    }
    input[type="checkbox"]:checked::before {
        content: '✔';
        color: var(--checkbox-check-color, white);
        font-size: 14px;
    }
    label {
        color: var(--checkbox-label-color, #333);
        font-size: 16px;
        text-align: center;
    }
    `;

    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
    }

    connectedCallback() {
        this.render();
    }

    render() {
        const style = document.createElement('style');
        style.textContent = CustomCheckbox.css;

        const container = document.createElement('div');
        container.className = 'checkbox-container';

        const checkbox = document.createElement('input');
        checkbox.type = 'checkbox';

        const label = document.createElement('label');
        label.textContent = this.getAttribute('label') || 'Checkbox Label';

        container.append(checkbox, label);
        this.shadowRoot.append(style, container);
    }
}

customElements.define(CustomCheckbox.TAG, CustomCheckbox);