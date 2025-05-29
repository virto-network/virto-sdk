import { IStorage } from "./IStorage";

export class LocalStorageImpl<T> implements IStorage<T> {
    private storageKey: string;

    constructor(storageKey: string = "sessions") {
        this.storageKey = storageKey;
    }

    store(key: string, session: T): void {
        const sessions = this.getAllFromStorage();
        sessions[key] = session;
        localStorage.setItem(this.storageKey, JSON.stringify(sessions));
    }

    get(key: string): T | null {
        const sessions = this.getAllFromStorage();
        return sessions[key] || null;
    }

    getAll(): T[] {
        const sessions = this.getAllFromStorage();
        return Object.values(sessions);
    }

    remove(key: string): boolean {
        const sessions = this.getAllFromStorage();
        if (key in sessions) {
            delete sessions[key];
            localStorage.setItem(this.storageKey, JSON.stringify(sessions));
            return true;
        }
        return false;
    }

    clear(): void {
        localStorage.removeItem(this.storageKey);
    }

    private getAllFromStorage(): Record<string, T> {
        const saved = localStorage.getItem(this.storageKey);
        return saved ? JSON.parse(saved) : {};
    }
} 