import { sube } from 'sube-js';
import  { default as Initwallet, JsWallet } from '@virtonetwork/libwallet';

window.sube = sube;
export function setupSign(element: HTMLButtonElement) {
  let counter = 0
  const setCounter = async (count: number) => {

    const mnomic = document.querySelector<HTMLInputElement>('#mnomic')?.value ?? '';
    const uri = document.querySelector<HTMLInputElement>('#uri')?.value ?? '';
    console.log("this is the uri: ", uri);
    const body = JSON.parse(document.querySelector<HTMLInputElement>('#data')?.value ?? '');
    console.log("wallet before init");
    await Initwallet();
    console.log("wallet init");

    const wallet = new JsWallet({
      Simple: mnomic,
    }, "");
    console.log("after create jswallet");
    await wallet.unlock({});

    await sube(uri, {
      body,
      from: wallet.getAddress().repr,
      sign: (message: Uint8Array) => wallet.sign(message),
    });

    counter = count
    element.innerHTML = `Tx is ${counter}`
  }

  element.addEventListener('click', () => setCounter(counter + 1));
}
