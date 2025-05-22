import Wallet from "./wallet";
import { WalletFactory } from "./factory/walletFactory";
import { Command, WalletType } from "./types";

interface ServerSession {
    userId: string;
    createdAt: number;
    address: string;
    wallet: Wallet;
    walletType: WalletType;
    mnemonic: string;
}

export default class ServerManager {
    private sessions: Map<string, ServerSession> = new Map();

    constructor(private walletFactory: WalletFactory) { }

    async create(command: Command, userId: string, walletType: WalletType, mnemonic?: string) {
        const walletImpl = this.walletFactory.create(walletType, mnemonic);

        const wallet = new Wallet(walletImpl);

        await wallet.sign(command);

        const address = await wallet.getAddress();

        const session: ServerSession = {
            userId,
            createdAt: Date.now(),
            address,
            wallet,
            walletType,
            mnemonic: walletImpl.getMnemonic()
        };

        this.sessions.set(JSON.stringify(userId), session);

        return {
            ok: true,
            session
        };
    }

    getWallet(userId: string): Wallet | null {
        const session = this.sessions.get(JSON.stringify(userId));
        return session?.wallet || null;
    }

    getSession(userId: string): ServerSession | undefined {
        return this.sessions.get(JSON.stringify(userId));
    }

    getAllSessions(): ServerSession[] {
        return Array.from(this.sessions.values());
    }

    removeSession(userId: string): boolean {
        return this.sessions.delete(JSON.stringify(userId));
    }
} 