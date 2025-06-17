import { PepperHandler, PepperType, PepperConfig, PepperData } from "../types";
import { CodePepperHandler } from "./codePepperHandler";
import { VError } from "../utils/error";

export class PepperManager {
    private handlers: Map<PepperType, PepperHandler>;
    private config: PepperConfig | null;

    constructor(config?: PepperConfig) {
        this.handlers = new Map();
        this.config = config || null;
        
        this.registerDefaultHandlers();
    }

    private registerDefaultHandlers(): void {
        this.registerHandler(new CodePepperHandler());
    }

    public registerHandler(handler: PepperHandler): void {
        this.handlers.set(handler.type, handler);
    }

    public getHandler(type: PepperType): PepperHandler | undefined {
        return this.handlers.get(type);
    }

    public getConfiguredPepperType(): PepperType | null {
        return this.config?.type || null;
    }

    public isPepperRequired(): boolean {
        return this.config !== null;
    }

    public validatePepper(pepperData: PepperData): boolean {
        const handler = this.getHandler(pepperData.type);
        if (!handler) {
            throw new VError("E_INVALID_PEPPER_TYPE", `Unsupported pepper type: ${pepperData.type}`);
        }

        return handler.validate(pepperData.value);
    }

    public preparePepper(pepperData: PepperData): PepperData {
        const handler = this.getHandler(pepperData.type);
        if (!handler) {
            throw new VError("E_INVALID_PEPPER_TYPE", `Unsupported pepper type: ${pepperData.type}`);
        }

        const preparedValue = handler.prepare ? handler.prepare(pepperData.value) : pepperData.value;
        
        if (!handler.validate(preparedValue)) {
            throw new VError("E_INVALID_PEPPER_VALUE", `Invalid pepper value for type ${pepperData.type}`);
        }

        return {
            type: pepperData.type,
            value: preparedValue
        };
    }

    public getSupportedPepperTypes(): PepperType[] {
        return Array.from(this.handlers.keys());
    }

    public getConfiguration(): PepperConfig | null {
        return this.config;
    }
} 