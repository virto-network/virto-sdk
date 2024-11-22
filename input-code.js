class VerificationCodeInput extends HTMLElement {
    static TAG = 'input-code'
    static css = `
    :host {
        display: flex;
        width: 100%;
        gap: 8px;
    }
    input {
        flex: 1;
        min-width: 0;
        box-sizing: border-box;
        height: 68px;
        border-radius: 12px;
        padding: 0;
        border: 1px solid var(--color-accent, #ccc);
        text-align: center;
        font-size: 1.2em;
        -moz-appearance: textfield;
        &:focus-visible {
            outline: 2px solid var(--color-alt-rgb);
        }
    }
    input::-webkit-outer-spin-button,
    input::-webkit-inner-spin-button {
        -webkit-appearance: none;
        margin: 0;
    }
    `;

    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
    }

    connectedCallback() {
        this.render();
        this.setupEventListeners();
    }

    render() {
        const style = document.createElement("style");
        style.textContent = VerificationCodeInput.css;

        const inputs = Array.from({ length: 6 }, () => {
            const input = document.createElement("input");
            input.type = "number";
            input.min = 0;
            input.max = 9;
            input.required = true;
            input.autocomplete = "off";
            input.inputMode = "numeric";
            return input;
        });

        this.shadowRoot.append(style, ...inputs);
    }

    setupEventListeners() {
        const inputs = this.shadowRoot.querySelectorAll('input');

        inputs.forEach((input, index) => {
            input.addEventListener('input', (e) => {
                e.target.value = e.target.value.slice(0, 1);
                if (e.target.value.length === 1 && index < inputs.length - 1) {
                    inputs[index + 1].focus();
                }
            });

            input.addEventListener('keydown', (e) => {
                if (e.key === 'Backspace' && e.target.value.length === 0 && index > 0) {
                    inputs[index - 1].focus();
                }
            });
        });

        this.addEventListener('paste', this.handlePaste.bind(this));
    }

    handlePaste(e) {
        e.preventDefault();
        const pastedText = (e.clipboardData || window.clipboardData).getData('text');
        const inputs = this.shadowRoot.querySelectorAll('input');
        const digits = pastedText.replace(/\D/g, '').split('').slice(0, 6);

        inputs.forEach((input, index) => {
            input.value = digits[index] || '';
        });

        if (digits.length > 0) {
            inputs[Math.min(digits.length, 5)].focus();
        }
    }

    getValue() {
        return Array.from(this.shadowRoot.querySelectorAll('input'))
            .map(input => input.value)
            .join('');
    }

    reset() {
        this.shadowRoot.querySelectorAll('input').forEach(input => {
            input.value = '';
        });
        this.shadowRoot.querySelector('input').focus();
    }
}

customElements.define(VerificationCodeInput.TAG, VerificationCodeInput);