class Input extends HTMLElement {
    static TAG = 'input-virto'
    static css = `
    :host {
        display: block;
        width: 100%;
    }
    input {
        width: 100%;
        box-sizing: border-box;
        line-height: 28px;
        border-radius: 12px;
        padding: 1em;
        border: 1px solid var(--color-accent);
    }
    input:focus {
        outline: 1px solid var(--color-fill-btn);
    }
    input:invalid {
        border-color: red;
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
        const style = document.createElement("style");
        const input = document.createElement("input");
        
        input.type = this.getAttribute('type') || 'text';
        if (this.hasAttribute('required')) {
            input.required = true;
        }
        if (this.hasAttribute('aria-placeholder')) {
            input.placeholder = this.getAttribute('aria-placeholder');
        }

        style.textContent = Input.css;

        this.shadowRoot.append(style, input);
    }
}

customElements.define(Input.TAG, Input);