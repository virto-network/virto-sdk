class Input extends HTMLElement {
    static TAG = 'virto-input'
    static css = `
    :host {
        display: block;
        width: 100%;
    }
    input, textarea {
        width: 100%;
        box-sizing: border-box;
        line-height: 28px;
        border-radius: 12px;
        padding: 1em;
        border: 1px solid var(--color-accent);
        font-family: Outfit, sans-serif;
    }
    input:focus {
        outline: 1px solid var(--color-fill-btn);
    }
    input:invalid {
        border-color: red;
    }
    textarea {
        resize: vertical;
        min-height: 100px;
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
        const type = this.getAttribute('type') || 'text';
        const placeholder = this.getAttribute('aria-placeholder') || '';
        const required = this.hasAttribute('required');

        this.shadowRoot.innerHTML = `
            <style>${Input.css}</style>
            ${type === 'textarea' 
                ? `<textarea placeholder="${placeholder}" ${required ? 'required' : ''}></textarea>`
                : `<input type="${type}" placeholder="${placeholder}" ${required ? 'required' : ''}>`
            }
        `;
    }
}

customElements.define(Input.TAG, Input);