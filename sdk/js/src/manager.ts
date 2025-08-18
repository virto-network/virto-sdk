import Wallet from "./wallet";
import { WalletFactory } from "./factory/walletFactory";
import { Command, WalletType } from "./types";
import { IStorage, LocalStorageImpl } from "./storage";

export interface Session {
    userId: string;
    createdAt: number;
    address: string;
    wallet: Wallet | null;
    walletType: WalletType;
    mnemonic: string;
}

export default class SessionManager {
    private storage: IStorage<Session>;

    constructor(
        private walletFactory: WalletFactory, 
        storage?: IStorage<Session>
    ) {
        this.storage = storage || new LocalStorageImpl<Session>("sessions");
    }

    async create(command: Command, userId: string, walletType: WalletType, mnemonic?: string) {
        const walletImpl = this.walletFactory.create(walletType, mnemonic);

        const wallet = new Wallet(walletImpl);

        await wallet.sign(command);

        const address = await wallet.getAddress();

        const session: Session = {
            userId,
            createdAt: Date.now(),
            address,
            wallet: null as any, // Don't store the wallet object, it will be reconstructed
            walletType,
            mnemonic: walletImpl.getMnemonic()
        };

        const key = JSON.stringify(userId);
        await this.storage.store(key, session);

        return {
            ok: true,
            session: {
                ...session,
                wallet
            }
        };
    }

    async getWallet(userId: string): Promise<Wallet | null> {
        console.log({ userId: JSON.stringify(userId) });
        const key = JSON.stringify(userId);
        const session = await this.storage.get(key);
        console.log({ session });
        
        if (!session) {
            return null;
        }

        const walletImpl = this.walletFactory.create(session.walletType, session.mnemonic);
        const wallet = new Wallet(walletImpl);
        
        return wallet;
    }

    async getSession(userId: string): Promise<Session | null> {
        const key = JSON.stringify(userId);
        return await this.storage.get(key);
    }

    async getAllSessions(): Promise<Session[]> {
        return await this.storage.getAll();
    }

    async removeSession(userId: string): Promise<boolean> {
        const key = JSON.stringify(userId);
        return await this.storage.remove(key);
    }
}
