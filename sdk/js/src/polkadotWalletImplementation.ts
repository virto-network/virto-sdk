import { ApiPromise, Keyring, WsProvider } from "@polkadot/api";
import { IWalletImplementation } from "./IWalletImplementation";
import { KeyringPair } from "@polkadot/keyring/types";
import { hexToU8a } from "@polkadot/util";
import { signSendAndWait } from "./utils/signer";
import { mnemonicGenerate } from '@polkadot/util-crypto';
import { Command } from "./types";

export class PolkadotWalletImplementation implements IWalletImplementation {
    private signer: KeyringPair;
    private isUnlocked: boolean = false;
    private mnemonic: string;

    constructor(mnemonic: string | null = null) {
        const keyring = new Keyring({ type: "sr25519", ss58Format: 2 });
        console.log({ mnemonic })
        const m = mnemonic ? mnemonic : mnemonicGenerate();
        console.log({ m })
        this.mnemonic = m;
        this.signer = keyring.addFromUri(m);
    }

    async unlock(): Promise<void> {
        if (!this.isUnlocked) {
            this.isUnlocked = true;
        }
    }

    async getAddress(): Promise<string> {
        return this.signer.address;
    }

    async sign(command: Command): Promise<boolean> {
        if (!this.isUnlocked) {
            await this.unlock();
        }

        const wsProvider = new WsProvider("ws://localhost:12281");
        const api = await ApiPromise.create({ provider: wsProvider });
        console.log({ command })
        const extrinsic = api.tx(hexToU8a(command.hex));

        console.log({ signer: this.getAddress() })
        let result = await signSendAndWait(extrinsic, this.signer);

        if (!result) {
            throw new Error("Signing with polkadot failed");
        }

        return !!result;
    }

    getMnemonic(): string {
        return this.mnemonic;
    }
}
