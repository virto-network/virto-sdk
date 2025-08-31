import 'es-arraybuffer-base64/Uint8Array.fromBase64/auto';

export function fromBase64Url(str: string): ArrayBuffer {
    return Uint8Array.fromBase64(str, { alphabet: 'base64url' }) as unknown as ArrayBuffer;
}
