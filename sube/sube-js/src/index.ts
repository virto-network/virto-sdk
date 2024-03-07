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
    call: {
      nonce: options.nonce,
      body: options.body
    },
    from: options.from,
  }, options ? function (i: Uint8Array) {
    return options.sign(i);
  } : () => { }) as Promise<T>;
}
