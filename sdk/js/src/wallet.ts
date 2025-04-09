import { JsWallet } from '@virtonetwork/libwallet';
import { sube } from '@virtonetwork/sube';
import { Command } from './types';
import { IWalletImplementation } from './IWalletImplementation';

export type SubeFn = typeof sube;
export type JsWalletBuilder = (mnemonic: string | null) => InstanceType<typeof JsWallet>;

export default class Wallet {
  constructor(private walletImpl: IWalletImplementation) { }

  async unlock(): Promise<void> {
    return this.walletImpl.unlock();
  }

  async getAddress(): Promise<string> {
    return this.walletImpl.getAddress();
  }

  async sign(command: Command): Promise<boolean> {
    return this.walletImpl.sign(command);
  }
}
