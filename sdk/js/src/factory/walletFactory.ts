import { WalletType } from "../types";
import { IWalletImplementation } from "../IWalletImplementation";

export interface WalletFactory {
  create: (walletType: WalletType, mnemonic?: string) => IWalletImplementation;
}
export { WalletType };
