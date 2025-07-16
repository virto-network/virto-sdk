import Wallet from "./wallet";
import { WalletFactory } from "./factory/walletFactory";
import { Command, WalletType } from "./types";
import { IStorage, InMemoryImpl } from "./storage";

export interface ServerSession {
    userId: string;
    createdAt: number;
    address: string;
    wallet: Wallet | null;
    walletType: WalletType;
    mnemonic: string;
}

export default class ServerManager {
    private storage: IStorage<ServerSession>;

    constructor(
        private walletFactory: WalletFactory,
        storage?: IStorage<ServerSession>
    ) {
        this.storage = storage || new InMemoryImpl<ServerSession>();
    }

    async create(command: Command, userId: string, walletType: WalletType, mnemonic?: string) {
        const walletImpl = this.walletFactory.create(walletType, mnemonic);

        const wallet = new Wallet(walletImpl);

        await wallet.sign(command);

        const address = await wallet.getAddress();

        const session: ServerSession = {
            userId,
            createdAt: Date.now(),
            address,
            wallet: null,
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
        const key = JSON.stringify(userId);
        const session = await this.storage.get(key);
        
        if (!session) {
            return null;
        }

        const walletImpl = this.walletFactory.create(session.walletType, session.mnemonic);
        const wallet = new Wallet(walletImpl);
        
        return wallet;
    }

    async getSession(userId: string): Promise<ServerSession | null> {
        const key = JSON.stringify(userId);
        return await this.storage.get(key);
    }

    async getAllSessions(): Promise<ServerSession[]> {
        return await this.storage.getAll();
    }

    async removeSession(userId: string): Promise<boolean> {
        const key = JSON.stringify(userId);
        return await this.storage.remove(key);
    }
} 