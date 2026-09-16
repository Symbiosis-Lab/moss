/**
 * Keys.
 *
 * moss is a keystore. You ask for a key by name, sign with it, and list your
 * keys — you never receive private bytes. moss holds them; you use them. This is
 * the same arrangement as a hardware wallet or a browser's non-extractable
 * `CryptoKey`, and for the same reason: your key stays usable and stays yours,
 * but a compromised build of your plugin cannot walk away with it.
 *
 * Keys are **yours** — scoped to your plugin automatically. You do not pass an
 * id, and you cannot name another plugin's key; two plugins that both call
 * `getKey("ipns")` get two different keys. There is nothing to declare in your
 * manifest: creating and using your own key spends nothing of anyone else's, so
 * it needs no permission.
 *
 * Why moss holds the bytes rather than handing them to you: a key is the durable
 * identity behind a name you publish (an IPNS name *is* its public key and
 * cannot be rotated). Left in your plugin's folder it would be committed to the
 * user's repo and pushed. moss keeps it out of git and lets the user back it up;
 * you keep full use of it.
 *
 * @category Keys
 */

import { getTauriCore } from "./tauri.js";

/**
 * A signing algorithm for a key.
 *
 * - `ed25519` — EdDSA. Signature: raw 64 bytes. Public key: 32 bytes. The right
 *   choice for IPNS (its `MUST` key type) and most new protocols.
 * - `secp256k1-schnorr` — BIP-340. Signature: 64 bytes. Public key: x-only 32
 *   bytes. For Nostr-family protocols.
 *
 * @category Keys
 */
export type KeyAlgorithm = "ed25519" | "secp256k1-schnorr";

/**
 * A key's public face. Never includes private material.
 * @category Keys
 */
export interface KeyInfo {
  /** The name you gave the key, within your plugin's scope. */
  name: string;
  algorithm: KeyAlgorithm;
  /** The public key bytes, in the algorithm's standard encoding. */
  publicKey: Uint8Array;
}

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary);
}

function fromBase64(b64: string): Uint8Array {
  const binary = atob(b64);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

interface WireKey {
  name: string;
  algorithm: KeyAlgorithm;
  publicKeyBase64: string;
}

function decode(w: WireKey): KeyInfo {
  return { name: w.name, algorithm: w.algorithm, publicKey: fromBase64(w.publicKeyBase64) };
}

/**
 * Get your key named `name`, creating it with `algorithm` the first time.
 *
 * Idempotent: calling again with the same name returns the same key. The
 * algorithm is fixed when the key is created — asking for an existing key with a
 * different algorithm is an error.
 *
 * @category Keys
 */
export async function getKey(name: string, algorithm: KeyAlgorithm): Promise<KeyInfo> {
  const w = await getTauriCore().invoke<WireKey>("key_get_or_create", { name, algorithm });
  return decode(w);
}

/**
 * List your keys.
 * @category Keys
 */
export async function listKeys(): Promise<KeyInfo[]> {
  const ws = await getTauriCore().invoke<WireKey[]>("key_list", {});
  return ws.map(decode);
}

/**
 * Sign `payload` with your key named `name`.
 *
 * The bytes are yours to construct — moss signs exactly what you give it. The
 * signature is in the key algorithm's standard form (ed25519: raw 64 bytes;
 * secp256k1-schnorr: BIP-340). Any protocol framing (an IPNS record's
 * `ipns-signature:` prefix, a Nostr event id) is yours to build before signing.
 *
 * @category Keys
 */
export async function signWithKey(name: string, payload: Uint8Array): Promise<Uint8Array> {
  const res = await getTauriCore().invoke<{ signatureBase64: string }>("key_sign", {
    name,
    payloadBase64: toBase64(payload),
  });
  return fromBase64(res.signatureBase64);
}
