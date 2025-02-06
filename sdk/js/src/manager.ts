import { JsWallet } from '@virtonetwork/libwallet';
import { sube } from '@virtonetwork/sube';

export class SessionManager {
    private wallet: JsWallet;
    private isUnlocked: boolean = false;

    constructor() {
        this.wallet = new JsWallet({ Simple: null });
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

    async createSession(extrinsic: Record<string, any>) {
        if (!this.isUnlocked) {
            await this.unlock();
        }

        const subeFn = (typeof window !== "undefined" && window.sube)
            ? window.sube
            : sube;

        const result = await subeFn("https://kreivo.io/balances/transfer_keep_alive", {
            body: extrinsic,
            from: this.wallet.getAddress().repr,
            sign: (message: Uint8Array) => this.wallet.sign(message)
        });

        if (!result) {
            throw new Error("Session creation failed");
        }

        this.persistWallet();

        return {
            ok: true,
        };
    }

    private persistWallet() {
        localStorage.setItem('wallet', JSON.stringify({
            address: this.getAddress(),
        }));
    }
}
