class LinkAuthenticate extends HTMLElement {
    static TAG = 'link-authenticate';
    static css = `
    :host {
        display: block;
        width: 100%;
    }
    .copy-container {
        display: flex;
        align-items: center;
        justify-content: space-between;
        width: 100%;
        box-sizing: border-box;
        line-height: 28px;
        border-radius: 35px;
        padding: 0.5em 1em;
        border: 1px solid var(--color-accent);
        background-color: var(--color-input);
        font-family: inherit;
        font-size: inherit;
        color: var(--color-text, black);
        cursor: pointer;
        transition: all 0.3s ease;
    }
    .copy-container:hover, .copy-container:focus-within {
        border-color: var(--color-accent, #ccc);
        outline: none;
    }
    .copy-container:active {
        background-color: var(--color-accent-light, #e0e0e0);
    }
    .link {
        flex-grow: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        text-decoration: none;
        color: var(--color-text, black);
    }
    .icon {
        margin-left: 8px;
        cursor: pointer;
        user-select: none;
    }
    .icon svg {
        width: 20px;
        height: 20px;
        fill: currentColor;
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
        const style = document.createElement('style');
        style.textContent = LinkAuthenticate.css;

        const container = document.createElement('div');
        container.className = 'copy-container';
        container.tabIndex = 0;

        const link = document.createElement('span');
        link.className = 'link';
        link.textContent = this.getAttribute('link') || 'https://virto.network/invoice/go/lkewiow91TIB4235f23fd';

        const icon = document.createElement('span');
        icon.className = 'icon';
        icon.innerHTML = `
            <svg viewBox="0 0 24 24">
                <path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z"/>
            </svg>
        `;

        container.append(link, icon);
        this.shadowRoot.append(style, container);
    }

    setupEventListeners() {
        const container = this.shadowRoot.querySelector('.copy-container');
        const link = this.shadowRoot.querySelector('.link');

        container.addEventListener('click', (e) => {
            if (e.target.closest('.icon')) {
                this.copyToClipboard(link.textContent);
            }
        });
    }

    copyToClipboard(text) {
        navigator.clipboard.writeText(text).then(() => {
            this.showTooltip('Copied!');
        }).catch(() => {
            this.showTooltip('Failed to copy');
        });
    }

    showTooltip(message) {
        const tooltip = document.createElement('div');
        tooltip.textContent = message;
        tooltip.style.cssText = `
            position: absolute;
            background: #333;
            color: white;
            padding: 5px 10px;
            border-radius: 5px;
            font-size: 14px;
            top: 100%;
            left: 50%;
            transform: translateX(-50%);
            opacity: 0;
            transition: opacity 0.3s;
        `;
        
        this.shadowRoot.querySelector('.copy-container').appendChild(tooltip);
        
        tooltip.offsetHeight;
        
        tooltip.style.opacity = '1';
        
        setTimeout(() => {
            tooltip.style.opacity = '0';
            tooltip.addEventListener('transitionend', () => tooltip.remove());
        }, 2000);
    }
}

customElements.define(LinkAuthenticate.TAG, LinkAuthenticate);