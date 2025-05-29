export interface IStorage<T> {
    /**
     * Store a session with the given key
     * @param key - The unique identifier for the session
     * @param session - The session data to store
     */
    store(key: string, session: T): Promise<void> | void;

    /**
     * Retrieve a session by key
     * @param key - The unique identifier for the session
     * @returns The session data or null if not found
     */
    get(key: string): Promise<T | null> | T | null;

    /**
     * Get all stored sessions
     * @returns Array of all sessions
     */
    getAll(): Promise<T[]> | T[];

    /**
     * Remove a session by key
     * @param key - The unique identifier for the session
     * @returns true if the session was removed, false if it didn't exist
     */
    remove(key: string): Promise<boolean> | boolean;

    /**
     * Clear all sessions
     */
    clear(): Promise<void> | void;
} 