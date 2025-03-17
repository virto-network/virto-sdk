export type BaseProfile = {
    id: string;
    name: string;
    displayName: string;
};

export type User<Profile, Metadata extends Record<string, unknown>> = {
    profile: Profile;
    metadata: Metadata;
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
