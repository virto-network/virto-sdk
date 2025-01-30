export class VError extends Error {
    constructor(public code: string, message: string) {
        super(message);
        this.name = "VError";

        Object.setPrototypeOf(this, VError.prototype);
    }
}
