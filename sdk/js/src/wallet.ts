import { JsWallet } from '@virtonetwork/libwallet';
import { sube } from '@virtonetwork/sube';
import { Command } from './auth';

export type SubeFn = typeof sube;
export type JsWalletBuilder = (mnemonic: string | null) => InstanceType<typeof JsWallet>;

export default class Wallet {
    private wallet: JsWallet;
    private isUnlocked: boolean = false;

    constructor(mnemonic: string | null = null, private subeFn: SubeFn, private JsWalletFn: JsWalletBuilder) {
        this.wallet = this.JsWalletFn(mnemonic);
    }

    async unlock(): Promise<void> {
        if (!this.isUnlocked) {
            await this.wallet.unlock(null, null);
            this.isUnlocked = true;
        }
    }

    async getAddress(): Promise<string> {
        if (!this.isUnlocked) {
            await this.unlock();
        }
        return this.wallet.getAddress().toHex();
    }

    async sign(command: Command): Promise<any> {
        if (!this.isUnlocked) {
            await this.unlock();
        }

        const result = await this.subeFn(command.url, {
            body: command.body,
            from: await this.wallet.getAddress().repr,
            sign: (message: Uint8Array) => this.wallet.sign(message)
        });
        if (!result) {
            throw new Error("Signing with sube failed");
        }

        return result;
    }

    toJSON(): string {
        return JSON.stringify({
            address: this.getAddress(),
            mnemonic: this.wallet.phrase,
            isUnlocked: this.isUnlocked
        });
    }
}
