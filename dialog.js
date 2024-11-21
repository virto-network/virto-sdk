class DialogoModal extends HTMLElement {
    constructor() {
        super();
        this.attachShadow({ mode: 'open' });
        this.currentStep = 1;
    }

    connectedCallback() {
        this.render();
        this.setupEventListeners();
    }

    get totalSteps() {
        return this.querySelectorAll('[slot^="step-"]').length;
    }

    render() {
        this.shadowRoot.innerHTML = `
        <style>
            :host {
                all: initial;
                display: none;
                position: fixed;
                top: 0;
                left: 0;
                width: 100%;
                height: 100%;
                background-color: #0000004D;
                justify-content: center;
                align-items: center;
                opacity: 0;
                transition: opacity 0.3s ease;
            }

            :host(.visible) {
                display: flex;
                opacity: 1;
            }

            .dialog {
                display: flex;
                flex-direction: column;
                gap: 1em;
                font-family: Outfit, sans-serif;
                width: 100%;
                max-width: 528px;
                height: fit-content;
                background: linear-gradient(0deg, rgba(231, 247, 232, 0.5), rgba(231, 247, 232, 0.5)),
                            radial-gradient(84.04% 109.28% at 10.3% 12.14%, rgba(86, 201, 96, 0.5) 0%, rgba(198, 235, 199, 0) 98.5%);
                border-radius: 12px;
                box-shadow: 0px 2px var(--Blurblur-3, 3px) -1px rgba(26, 26, 26, 0.08),
                            0px 1px var(--Blurblur-0, 0px) 0px rgba(26, 26, 26, 0.08);
                backdrop-filter: blur(32px);
                padding: 1em;
                gap: clamp(4px, 1vw, var(--spacing7, 14px));
                transform: translateX(100%);
                opacity: 0; /* Start hidden */
                pointer-events: none; /* Prevent interactions */
                transition: transform 0.3s ease, opacity 0.2s ease;
            }
            
            :host(.visible) .dialog {
                transform: translateX(0);
                opacity: 1;
                pointer-events: auto;
                animation: slideInRight 0.5s forwards;
            }
            
            :host(.hidden) .dialog {
                transform: translateX(-100%);
                opacity: 0;
                pointer-events: none;
                animation: slideOutLeft 0.3s forwards;
            }
            
            @keyframes slideInRight {
                from {
                    transform: translateX(100%);
                    opacity: 0;
                }
                to {
                    transform: translateX(0);
                    opacity: 1;
                }
            }
            
            @keyframes slideOutLeft {
                from {
                    transform: translateX(0);
                    opacity: 1;
                }
                50% {
                    opacity: 0;
                }
                to {
                    transform: translateX(-100%);
                    opacity: 0;
                }
            }

            header {
                display: flex;
                align-items: center;
                gap: 1em;
            }

            header h2 {
                font-size: 1.4em;
                font-weight: 600;
                color: var(--color-txt);
                margin: 0;
            }

            hr {
                border: none;
                border-radius: 1px;
                border-top: 1px solid var(--color-accent);
                margin: 1em 0;
            }

            .navigation {
                box-sizing: border-box;
                justify-content: space-between;
                display: flex;
                width: 100%;
                flex-direction: row;
                color: var(--color-txt);
            }

            .step-content {
                display: none;
                opacity: 0;
                transition: opacity 0.3s ease;
            }
            
            .step-content.active {
                display: block;
                opacity: 1;
            }

            .image-step-1 {
                position: absolute;
                top: 0;
                right: 0;
                width: 200px;
                height: 200px;
                object-fit: contain;
                margin: auto 0;
            }

            .center-image {
                display: flex;
                justify-content: center;
                align-items: center;
                width: 200px;
                height: 200px;
                margin: 0 auto;
            }

            .qr-code {
                background-color: #fff;
            }

            .loader {
                width: 70px;
                height: 70px;
                border-radius: 50%;
                animation: spin 1s linear infinite;
                background: conic-gradient(from 0deg at 50% 50%, #F0FDF1 0%, rgba(255, 255, 255, 0) 100%);
                mask: radial-gradient(circle, transparent 60%, black 61%);
                -webkit-mask: radial-gradient(circle, transparent 60%, black 61%);
            }

            @keyframes spin {
                from { transform: rotate(0deg); }
                to { transform: rotate(360deg); }
            }

            .verificated {
                background-color: transparent;
            }
        </style>
        <div class="dialog">
            <header>
                <slot name="logo"></slot>
                <h2 id="step-title"></h2>
            </header>
            <hr>
            ${Array.from({ length: this.totalSteps }, (_, i) => i + 1).map(step => `
                <div class="step-content ${step === this.currentStep ? 'active' : ''}" data-step="${step}">
                    <div class="image-container"></div>
                    <slot name="step-${step}"></slot>
                </div>
            `).join('')}
            <div class="navigation">
                <button-virto id="prevButton" ?disabled="${this.currentStep === 1}"></button-virto>
                <button-virto id="nextButton"></button-virto>
            </div>
        </div>
        `;
        this.updateStepContent();
    }

    setupEventListeners() {
        this.shadowRoot.getElementById('prevButton').addEventListener('click', (e) => this.handleButtonClick(e, 'prev'));
        this.shadowRoot.getElementById('nextButton').addEventListener('click', (e) => this.handleButtonClick(e, 'next'));
    }

    handleButtonClick(event, direction) {
        const buttonLabel = event.target.getAttribute('label').toLowerCase();
        if (buttonLabel === 'cancel' || buttonLabel === 'close') {
            this.hide();
        } else {
            this.navigate(direction);
        }
    }

    navigate(direction) {
        const nextStep = direction === 'next' ? this.currentStep + 1 : this.currentStep - 1;

        if (nextStep < 1 || nextStep > this.totalSteps) {
            return;
        }

        this.currentStep = nextStep;
        this.updateStepContent();
        this.animateStepTransition(direction);
    }

    animateStepTransition(direction) {
        const dialog = this.shadowRoot.querySelector('.dialog');
        dialog.style.animation = direction === 'next' ? 'slideOutLeft 0.3s forwards' : 'slideInRight 0.3s forwards';
        
        dialog.addEventListener('animationend', () => {
            dialog.style.animation = direction === 'next' ? 'slideInRight 0.3s forwards' : 'slideOutLeft 0.3s forwards';
        }, { once: true });
    }

    updateStepContent() {
        const stepData = this.steps[this.currentStep - 1];
        this.shadowRoot.getElementById('step-title').textContent = stepData.title;
        
        const prevButton = this.shadowRoot.getElementById('prevButton');
        const nextButton = this.shadowRoot.getElementById('nextButton');

        if (stepData.singleButton) {
            prevButton.style.display = 'none';
            nextButton.setAttribute('label', stepData.singleButtonLabel);
            nextButton.style.width = '100%';
        } else {
            prevButton.style.display = '';
            prevButton.setAttribute('label', stepData.prevButtonLabel);
            nextButton.setAttribute('label', stepData.nextButtonLabel);
            nextButton.style.width = '';
        }

        prevButton.disabled = this.currentStep === 1;
        nextButton.disabled = this.currentStep === this.totalSteps && !stepData.singleButton;

        this.shadowRoot.querySelectorAll('.step-content').forEach(el => el.classList.remove('active'));
        this.shadowRoot.querySelector(`.step-content[data-step="${this.currentStep}"]`).classList.add('active');

        this.updateStepImage();
    }

    updateStepImage() {
        const imageContainer = this.shadowRoot.querySelector('.step-content.active .image-container');
        imageContainer.innerHTML = '';

        switch (this.currentStep) {
            case 1:
                imageContainer.innerHTML = '<img class="image-step-1" src="public/Welcome.svg" alt="Illustration">';
                break;
            case 4:
                imageContainer.innerHTML = '<img class="center-image qr-code" src="public/qr.jpg" alt="QR Code Placeholder">';
                break;
            case 5:
                imageContainer.innerHTML = '<div class="center-image loader"></div>';
                break;
            case 6:
                imageContainer.innerHTML = '<img class="center-image verificated" src="public/Ok.svg" alt="Authentication Successful">';
                break;
            default:
                break;
        }
    }

    show() {
        this.classList.add('visible');
    }

    hide() {
        const dialog = this.shadowRoot.querySelector('.dialog');
        dialog.style.animation = 'slideOutLeft 0.5s forwards';
        dialog.addEventListener('animationend', () => {
            this.classList.remove('visible');
            this.dispatchEvent(new CustomEvent('dialog-closed'));
        }, { once: true });
    }

    reset() {
        this.currentStep = 1;
        this.render();
        this.setupEventListeners();
    }

    get steps() {
        return [
            { title: "Virto requires you to signup", prevButtonLabel: "Cancel", nextButtonLabel: "Request Code" },
            { title: "Virto requires you to signup", prevButtonLabel: "Change Number", nextButtonLabel: "Continue" },
            { title: "Virto requires you to signup", prevButtonLabel: "Cancel", nextButtonLabel: "Continue" },
            { title: "Secure your account", prevButtonLabel: "Cancel", nextButtonLabel: "Continue" },
            { title: "Secure your account", singleButton: true, singleButtonLabel: "Cloe" },
            { title: "Secure your account", singleButton: true, singleButtonLabel: "Close" },
        ];
    }
}

customElements.define('dialog-virto', DialogoModal);