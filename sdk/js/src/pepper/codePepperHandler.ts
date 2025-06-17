import { PepperHandler, PepperType } from "../types";

export class CodePepperHandler implements PepperHandler {
    type = PepperType.CODE;
    private length: number;

    constructor(length: number = 4) {
        this.length = length;
    }

    validate(value: string): boolean {
        // Validate that the value is exactly the required length and contains only digits
        const regex = new RegExp(`^\\d{${this.length}}$`);
        return regex.test(value);
    }

    getDescription(): string {
        return `Enter a ${this.length}-digit numeric code`;
    }

    prepare(value: string): string {
        // Remove any whitespace and ensure it's a string
        return value.trim();
    }
} 