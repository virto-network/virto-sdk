import { sube } from 'sube-js';
import  { default as Initwallet, JsWallet } from '@virtonetwork/libwallet';

export function setupSign(element: HTMLButtonElement) {
  let counter = 0
  const setCounter = async (count: number) => {

    const mnomic = document.querySelector<HTMLInputElement>('#mnomic')?.value ?? '';
    const uri = document.querySelector<HTMLInputElement>('#uri')?.value ?? '';
    const body = JSON.parse(document.querySelector<HTMLInputElement>('#data')?.value ?? '');

    console.log({
      uri,
      mnomic,
      body
    });
    
    await Initwallet();

    const wallet = new JsWallet({
      Simple: mnomic,
    });
    
    await wallet.unlock({});

    await sube(uri, {
      body,
      from: wallet.getAddress().repr,
      sign: (message: Uint8Array) => wallet.sign(message)
    });

    counter = count
    element.innerHTML = `Tx is ${counter}`
  }

  element.addEventListener('click', () => setCounter(counter + 1))
  // setCounter(0)
}
