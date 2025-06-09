import { expect } from '@esm-bundle/chai';
import './notification.js';

describe('Rendering', () => {
  let element;

  beforeEach(() => {
    // TODO: Create it just once if possible and not problematic
    document.body.innerHTML = '';
    element = document.createElement('virto-notification');
    document.body.appendChild(element);
  });

  it('should render the notification component', () => {
    expect(element).to.exist;
    expect(element.tagName.toLowerCase()).to.equal('virto-notification');
  });

  it('should create a Shadow DOM root', () => {
    expect(element.shadowRoot).to.exist;
  });

  it('Shadow DOM should contain the <wa-callout> element', () => {
    const callout = element.shadowRoot.querySelector('wa-callout');
    expect(callout).to.exist;
    expect(callout.tagName.toLowerCase()).to.equal('wa-callout');
  });

  it('should test if the virto-notification component was defined', () => {
    const isDefined = window.customElements.get('virto-notification') !== undefined;
    expect(isDefined).to.be.true;
  });

  it('the component tag should start with virto-', () => {
    expect(element.tagName.toLowerCase().startsWith('virto-')).to.be.true;
  });

  it('should be able to take content using innerHTML', () => {
    element.innerHTML = 'Warning!';
    expect(element.innerHTML).to.contain('Warning!');
  });
});

describe('Attributes', () => {
  let element;

  beforeEach(() => {
    document.body.innerHTML = '';
    element = document.createElement('virto-notification');
    document.body.appendChild(element);
  });

  it('should take an attribute called variant', () => {
    element.setAttribute('variant', 'success');
    expect(element.getAttribute('variant')).to.equal('success');
  });

  it('should take an attribute called appearance', () => {
    element.setAttribute('appearance', 'accent');
    expect(element.getAttribute('appearance')).to.equal('accent');
  });

  it('should take an attribute called size', () => {
    element.setAttribute('size', 'small');
    expect(element.getAttribute('size')).to.equal('small');
  });
});

describe('Styles', () => {
  let element;

  beforeEach(() => {
    document.body.innerHTML = '';
    element = document.createElement('virto-notification');
    document.body.appendChild(element);
  });
  
  it('should change its style according to the value on variant attribute', () => {
    element.setAttribute('variant', 'success');
    const callout = element.shadowRoot.querySelector('wa-callout');
    const bgColor = window.getComputedStyle(callout).backgroundColor;
    expect(bgColor).to.be.oneOf(['rgb(144, 238, 144)', 'rgb(221, 251, 224)', 'rgb(218, 251, 221)']);
  });

  it('should add styles using adoptedStyleSheets', () => {
    expect(element.shadowRoot.adoptedStyleSheets.length).to.be.greaterThan(0);
  }); 
  
});

describe('Events', () => {
  let element;

  beforeEach(() => {
    document.body.innerHTML = '';
    element = document.createElement('virto-notification');
    document.body.appendChild(element);
  });

  it('responds to virto-info event');
  it('should accept the event virto-info');
  it('should accept the event virto-success');
  it('should accept the event virto-warning');
  it('should accept the event virto-danger');
});

describe('User Interactions', () => {
  it('should be triggered with the event virto-info by an action the user performed');
  it('should be triggered with the event virto-success by an action the user');
  it('should be triggered with the event virto-error by an action the user performed');
  it('should be triggered with the event virto-warning by an action the user performed');
});
