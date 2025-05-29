import { openDB, IDBPDatabase } from 'idb';
import { IStorage } from "../../../src/storage";

/**
 * Example IndexedDB implementation for client-side session storage using the 'idb' package
 * 
 * Usage:
 * ```typescript
 * import { SDK } from "@virto-sdk/js";
 * import { IndexedDBStorage } from "./storage/IndexedDBStorage";
 * 
 * const indexedDBStorage = new IndexedDBStorage('VirtoSessions', 'sessions');
 * 
 * const sdk = new SDK({
 *   storage: indexedDBStorage
 * });
 * ```
 */
export class IndexedDBStorage<T> implements IStorage<T> {
    private dbName: string;
    private storeName: string;
    private dbConnection: IDBPDatabase | null = null;

    constructor(dbName: string = 'VirtoSessions', storeName: string = 'sessions') {
        this.dbName = dbName;
        this.storeName = storeName;
    }

    private async getDB(): Promise<IDBPDatabase> {
        if (!this.dbConnection) {
            const storeName = this.storeName;
            this.dbConnection = await openDB(this.dbName, 1, {
                upgrade(db) {
                    if (!db.objectStoreNames.contains(storeName)) {
                        db.createObjectStore(storeName, { keyPath: 'id' });
                    }
                },
            });
        }
        return this.dbConnection;
    }

    async store(key: string, session: T): Promise<void> {
        const db = await this.getDB();
        await db.put(this.storeName, { id: key, data: session });
    }

    async get(key: string): Promise<T | null> {
        const db = await this.getDB();
        const result = await db.get(this.storeName, key);
        return result ? result.data : null;
    }

    async getAll(): Promise<T[]> {
        const db = await this.getDB();
        const results = await db.getAll(this.storeName);
        return results.map((item: any) => item.data);
    }

    async remove(key: string): Promise<boolean> {
        const db = await this.getDB();
        const existing = await db.get(this.storeName, key);
        
        if (existing) {
            await db.delete(this.storeName, key);
            return true;
        }
        return false;
    }

    async clear(): Promise<void> {
        const db = await this.getDB();
        await db.clear(this.storeName);
    }

    async close(): Promise<void> {
        if (this.dbConnection) {
            this.dbConnection.close();
            this.dbConnection = null;
        }
    }
} 