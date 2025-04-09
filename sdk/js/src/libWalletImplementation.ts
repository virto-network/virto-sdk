import { JsWallet } from "@virtonetwork/libwallet";
import { IWalletImplementation } from "./IWalletImplementation";
import { JsWalletBuilder, SubeFn } from "./wallet";
import { Command } from "./types";

export class LibWalletImplementation implements IWalletImplementation {
    private wallet: JsWallet;
    private isUnlocked: boolean = false;

    constructor(private subeFn: SubeFn, private JsWalletFn: JsWalletBuilder, mnemonic: string | null = null,) {
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

    async sign(command: Command): Promise<boolean> {
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
        return !!result;
    }
    getMnemonic(): string {
        return this.wallet.phrase;
    }
}
