import { IStorage } from "./IStorage";

export class InMemoryImpl<T> implements IStorage<T> {
    private storage: Map<string, T> = new Map();

    store(key: string, session: T): void {
        this.storage.set(key, session);
    }

    get(key: string): T | null {
        return this.storage.get(key) || null;
    }

    getAll(): T[] {
        return Array.from(this.storage.values());
    }

    remove(key: string): boolean {
        return this.storage.delete(key);
    }

    clear(): void {
        this.storage.clear();
    }
} 