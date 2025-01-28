import { JsWallet } from '../pkg/libwallet_js';

export type WalletConstructor = {
  Simple: string | null;
}

export class Wallet extends JsWallet {
  constructor(constructor: WalletConstructor);
  readonly address: PublicAddress;
}

export class PublicAddress extends Uint8Array {
  constructor(constructor: JsPublicWallet);
  toHex(): string;
}