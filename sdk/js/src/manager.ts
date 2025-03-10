import Wallet from "./wallet";
import { WalletFactory } from "./factory/walletFactory";
import { Command, WalletType } from "./types";

interface Session {
    userId: string;
    createdAt: number;
    address: string;
    wallet: Wallet;
    walletType: WalletType;
    mnemonic: string;
}

export default class SessionManager {
    private sessions: Map<string, Session> = new Map();

    constructor(private walletFactory: WalletFactory) {
        this.loadSessions();
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
            wallet,
            walletType,
            mnemonic: walletImpl.getMnemonic()
        };

        this.sessions.set(JSON.stringify(userId), session);
        this.persistSessions();

        return {
            ok: true,
            session
        };
    }

    getWallet(userId: string): Wallet | null {
        console.log({ userId: JSON.stringify(userId) });
        console.log({ this: this.sessions });
        const session = this.sessions.get(JSON.stringify(userId));
        console.log({ session });
        return session?.wallet || null;
    }

    getSession(userId: string): Session | undefined {
        return this.sessions.get(JSON.stringify(userId));
    }

    getAllSessions(): Session[] {
        return Array.from(this.sessions.values());
    }

    removeSession(userId: string): boolean {
        const removed = this.sessions.delete(JSON.stringify(userId));
        if (removed) {
            this.persistSessions();
        }
        return removed;
    }

    private persistSessions() {
        const entries = Array.from(this.sessions.entries());
        localStorage.setItem("sessions", JSON.stringify(entries));
    }

    private loadSessions() {
        const saved = localStorage.getItem("sessions");
        if (!saved) return;

        const entries: [string, Session][] = JSON.parse(saved);

        for (const [userId, s] of entries) {
            const walletImpl = this.walletFactory.create(s.walletType, s.mnemonic);
            const wallet = new Wallet(walletImpl);
            console.log({ wallet })

            const session: Session = {
                userId: JSON.parse(userId),
                createdAt: s.createdAt,
                address: s.address,
                wallet,
                walletType: s.walletType,
                mnemonic: s.mnemonic
            };

            const userIdString = JSON.stringify(JSON.parse(userId));
            this.sessions.set(userIdString, session);
        }
    }
}
