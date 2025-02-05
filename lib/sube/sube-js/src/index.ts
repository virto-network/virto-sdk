import { sube_js } from 'sube-js';

export interface SubeOptions {
  sign: (message: Uint8Array) => Promise<Uint8Array>,
  from: Uint8Array,
  body: any,
  nonce?: number,
}


export async function sube<T>(url: string, options?: SubeOptions) {
  return sube_js(url, options && {
    from: options.from,
    call: {
      nonce: options.nonce,
      body: options.body,
    }
  }, options ? function (i: Uint8Array) {
    return options.sign(i);
  } : () => {}) as Promise<T>;
}
