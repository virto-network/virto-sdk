import init, { sube_js } from '../pkg-web';


export interface SubeOptions {
  sign: (message: Uint8Array) => Uint8Array,
  from: Uint8Array,
  body: any,
  nonce?: number,
}

let inited = false;

export async function sube<T>(url: string, options?: SubeOptions) {
  if (!inited) {
    inited = true;
    await init();
  }

  return sube_js(url, options && {
    nonce: options.nonce,
    from: options.from,
    body: options.body,
  }, options ? function (i: Uint8Array) {
    return options.sign(i);
  } : () => {}) as Promise<T>;
}
