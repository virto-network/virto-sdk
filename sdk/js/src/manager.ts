import { JsWallet } from '@virtonetwork/libwallet';
// import { Keyring } from '@polkadot/api';
import { SubmittableExtrinsic } from '@polkadot/api/types';
// import { signSendAndWait } from "./utils/signAndSend";

export class SessionManager {
    private wallet: JsWallet;
    private isUnlocked: boolean = false;

    constructor() {
        this.wallet = new JsWallet({ Simple: null });
    }

    async unlock(): Promise<void> {
        // if (!this.isUnlocked) {
        //     await this.wallet.unlock(null, null);
        //     this.isUnlocked = true;
        // }
    }

    async getAddress(): Promise<string> {
        // if (!this.isUnlocked) {
        //     await this.unlock();
        // }
        // return this.wallet.getAddress().toHex();
        return "5G91111111111111111111111111111111111111111111111111111111111111";
    }

    async createSession(_: SubmittableExtrinsic<"promise">) {
        // if (!this.isUnlocked) {
        //     await this.unlock();
        // }

        // const signSendAndWaitFn = (typeof window !== "undefined" && window.signSendAndWait)
        //     ? window.signSendAndWait
        //     : signSendAndWait;

        // const result = await signSendAndWaitFn(extrinsic, this.wallet.keyPair);

        // const sessionCreated = (result as any).events.find(
        //     (record: { event: { method: string; }; }) => record.event.method === "SessionCreated"
        // );

        // if (!sessionCreated) {
        //     throw new Error("Session creation failed - SessionCreated event not found");
        // }

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
