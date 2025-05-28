import { IStorage } from "../../../src/storage";

/**
 * Example sessionStorage implementation for client-side session storage
 * Data will be cleared when the browser tab is closed
 * 
 * Usage:
 * ```typescript
 * import { SDK } from "@virto-sdk/js";
 * import { SessionStorageImpl } from "./storage/SessionStorageImpl";
 * 
 * const sessionStorage = new SessionStorageImpl('virto-sessions');
 * 
 * const sdk = new SDK({
 *   storage: sessionStorage
 * });
 * ```
 */
export class SessionStorageImpl<T> implements IStorage<T> {
    private storageKey: string;

    constructor(storageKey: string = "virto-sessions") {
        this.storageKey = storageKey;
    }

    store(key: string, session: T): void {
        const sessions = this.getAllFromStorage();
        sessions[key] = session;
        sessionStorage.setItem(this.storageKey, JSON.stringify(sessions));
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
            sessionStorage.setItem(this.storageKey, JSON.stringify(sessions));
            return true;
        }
        return false;
    }

    clear(): void {
        sessionStorage.removeItem(this.storageKey);
    }

    private getAllFromStorage(): Record<string, T> {
        const saved = sessionStorage.getItem(this.storageKey);
        return saved ? JSON.parse(saved) : {};
    }
} 