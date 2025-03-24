import { ApiPromise, Keyring, WsProvider } from "@polkadot/api";
import { IWalletImplementation } from "./IWalletImplementation";
import { KeyringPair } from "@polkadot/keyring/types";
import { hexToU8a } from "@polkadot/util";
import { signSendAndWait } from "./utils/signer";
import { mnemonicGenerate, cryptoWaitReady } from '@polkadot/util-crypto';
import { Command } from "./types";

export class PolkadotWalletImplementation implements IWalletImplementation {
    private signer!: KeyringPair;
    private isUnlocked: boolean = false;
    private mnemonic: string = "";
    private isInitialized: boolean = false;
    private initPromise: Promise<void>;
    private providerUrl: string;

    constructor(mnemonic: string | null = null, providerUrl: string) {
        this.initPromise = this.initialize(mnemonic);
        this.providerUrl = providerUrl;
    }

    private async initialize(mnemonic: string | null): Promise<void> {
        try {
            await cryptoWaitReady();
            
            const keyring = new Keyring({ type: "sr25519", ss58Format: 2 });
            console.log({ mnemonic });
            const m = mnemonic ? mnemonic : mnemonicGenerate();
            console.log({ m });
            this.mnemonic = m;
            this.signer = keyring.addFromUri(m);
            this.isInitialized = true;
        } catch (error) {
            console.error("Failed to initialize WASM:", error);
            throw new Error("The WASM interface has not been initialized. Ensure that you wait for the initialization Promise with waitReady() from @polkadot/wasm-crypto (or cryptoWaitReady() from @polkadot/util-crypto) before attempting to use WASM-only interfaces.");
        }
    }

    async unlock(): Promise<void> {
        await this.initPromise;
        
        if (!this.isUnlocked) {
            this.isUnlocked = true;
        }
    }

    async getAddress(): Promise<string> {
        await this.initPromise;
        
        return this.signer.address;
    }

    async sign(command: Command): Promise<boolean> {
        await this.initPromise;
        
        if (!this.isUnlocked) {
            await this.unlock();
        }

        const wsProvider = new WsProvider(this.providerUrl);
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
