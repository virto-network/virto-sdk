export type BaseProfile = {
    id: string;
    name?: string;
};

export type User<Profile> = {
    profile: Profile;
};

export type Command = {
    url: string;
    body: any;
    hex: string;
};

export enum WalletType {
    VIRTO = "virto",
    POLKADOT = "polkadot"
}

export enum PepperType {
    CODE = "code",
}

export interface PepperData {
    type: PepperType;
    value: string;
}

export interface PepperConfig {
    type: PepperType;
}

export interface PepperHandler {
    type: PepperType;
    validate(value: string): boolean;
    prepare?(value: string): string;
}

export interface CodePepperConfig extends PepperConfig {
    type: PepperType.CODE;
    length?: number; // Default 4
}
