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
                background-color: rgba(0, 0, 0, 0.5);
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
                transition: transform 0.3s ease;
            }

            :host(.visible) .dialog {
                transform: translateX(0);
                animation: slideInLeft 0.5s forwards;
            }

            :host(.hidden) {
                opacity: 0;
            }

            :host(.hidden) .dialog {
                transform: translateX(-100%);
                animation: slideOutLeft 0.5s forwards;
            }

            @keyframes 
                slideInLeft { 
                    0% { 
                        transform: translateX(100%); 
                        opacity: 0; 
                    } 
                    100% { 
                        transform: translateX(0); 
                        opacity: 1; 
                    } 
                } 
                
            @keyframes 
                slideOutLeft { 
                    0% { 
                        transform: translateX(0); 
                        opacity: 1; 
                    } 
                    100% 
                    { 
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
                transform: translateX(100%);
                transition: opacity 0.3s ease, transform 0.3s ease;
            }
            
            .step-content.active {
                display: block;
                opacity: 1;
                transform: translateX(0);
            }
        </style>
        <div class="dialog">
            <header>
                <slot name="logo"></slot>
                <h2 id="step-title"></h2>
            </header>
            <hr>
            ${Array.from({ length: this.totalSteps }, (_, i) => i + 1).map(step => `
                <div class="step-content ${step === this.currentStep ? 'active' : ''}">
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
        if (direction === 'prev' && this.currentStep > 1) {
            this.currentStep--;
        } else if (direction === 'next' && this.currentStep < this.totalSteps) {
            this.currentStep++;
        } else if (direction === 'next' && this.currentStep === this.totalSteps) {
            this.finish();
            return;
        }
        this.render();
        this.setupEventListeners();
        this.dispatchEvent(new CustomEvent('step-change', { detail: { step: this.currentStep } }));
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
    }

    finish() {
        this.dispatchEvent(new CustomEvent('register-complete'));
        this.hide();
    }

    show() {
        this.classList.add('visible');
    }

    hide() {
        this.classList.add('hidden');
        this.addEventListener('transitionend', () => {
            this.classList.remove('visible', 'hidden');
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
            { title: "Secure your account", singleButton: true, singleButtonLabel: "Clos" },
            { title: "Secure your account", singleButton: true, singleButtonLabel: "Close" },
        ];
    }
}

customElements.define('dialog-virto', DialogoModal);