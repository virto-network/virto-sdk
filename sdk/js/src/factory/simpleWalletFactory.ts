import { WalletFactory, WalletType } from "./walletFactory";
import { PolkadotWalletImplementation } from "../polkadotWalletImplementation";
import { SubeFn, JsWalletBuilder } from "../wallet";
import { IWalletImplementation } from "../IWalletImplementation";
import { LibWalletImplementation } from "../libWalletImplementation";

export class SimpleWalletFactory implements WalletFactory {
  constructor(
    private subeFn: SubeFn,
    private jsWalletFn: JsWalletBuilder,
    private providerUrl: string
  ) { }

  create(walletType: WalletType, mnemonic?: string): IWalletImplementation {
    if (walletType === WalletType.VIRTO) {
      return new LibWalletImplementation(this.subeFn, this.jsWalletFn, mnemonic);
    } else {
      return new PolkadotWalletImplementation(mnemonic, this.providerUrl);
    }
  }
}
