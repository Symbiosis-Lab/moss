/**
 * Keys and secrets — the two things moss holds for you.
 *
 * A **key** you never see: you ask for it by name and moss signs with it. A
 * **secret** you do see, because it is a token somebody else issued the user
 * and you have to put it in a header. Both are scoped to your plugin
 * automatically, both live outside the user's repo, and moss is the custodian
 * of both. That is why they share a file.
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
 * **You never draw a credential input.** moss asks the user for the token, in
 * moss's own modal, driven by the `setup.credentials` block in your manifest. A
 * plugin that draws its own password field is teaching users to type credentials
 * into whatever asks, which is the habit that makes phishing work — so the
 * registry refuses it. What you may do is *store what an authenticated flow
 * already returned to you*: that is `setSecret`. It refuses any key you declared
 * in `setup.credentials` or as a `config_schema` field of type `secret`, since
 * those are the ones the user typed and moss holds for them.
 *
 * The loop is git's fill → approve/reject, and the reject half is the one
 * plugins forget: a revoked token fails every publish identically until someone
 * says so. `rejectSecret` says so AND asks again, resolving with the
 * replacement, so your error path is catch → reject → retry rather than a
 * failed publish and an explanation.
 *
 * ```ts
 * let jwt = await moss.getSecret("pinata_jwt");
 * let res = await moss.fetch(url, { headers: { Authorization: `Bearer ${jwt}` } });
 * if (res.status === 401) {
 *   jwt = await moss.rejectSecret("pinata_jwt", { detail: "Pinata rejected this token." });
 *   if (!jwt) throw new Error("Publishing needs a Pinata token.");
 *   res = await moss.fetch(url, { headers: { Authorization: `Bearer ${jwt}` } });
 * }
 * ```
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

/**
 * Your secret stored under `key`, or `null` if there is none.
 *
 * `null` is the normal first-run answer, not an error. If your manifest
 * declares the key under `setup.credentials`, moss has already asked for it
 * before your hook ran, so `null` here means the user cancelled.
 *
 * @category Secrets
 */
export async function getSecret(key: string): Promise<string | null> {
  return await getTauriCore().invoke<string | null>("get_plugin_secret", { key });
}

/**
 * Tell moss the secret stored under `key` does not work — and get a replacement.
 *
 * Rejecting forgets the stored value and re-draws moss's own credential modal,
 * carrying your `detail` sentence as the reason — one sentence saying why you
 * are asking again, rendered above moss's own field. You supply the words and
 * no pixels. It resolves with the new value, or `null` if the user cancelled.
 * So the error path is a retry, not a failed publish with an explanation:
 *
 * ```ts
 * const fresh = await moss.rejectSecret("pinata_jwt", {
 *   detail: "Pinata says this token is no longer valid.",
 * });
 * if (fresh === null) return; // the user declined; stop, don't loop
 * ```
 *
 * Call this when the service rejects the credential itself — a 401, a revoked
 * token — not when a request merely failed. Rejecting on a network error throws
 * away a perfectly good token AND interrupts the user for nothing.
 *
 * The wait is the user's, so it is unbounded: moss's inactivity watchdog counts
 * an open credential modal as progress, not as a hung hook. With no window to
 * draw in — a headless `moss build` — the value is forgotten and `null` comes
 * back immediately, which is the honest answer when nobody can be asked.
 *
 * @category Secrets
 */
export async function rejectSecret(
  key: string,
  options?: { detail?: string }
): Promise<string | null> {
  return await getTauriCore().invoke<string | null>("reject_plugin_secret", {
    key,
    detail: options?.detail ?? null,
  });
}

/**
 * Store what an authenticated flow returned to you.
 *
 * **You may never draw the input yourself.** This is for a token you already
 * hold because a login moss supervised produced it — an OAuth redirect, a
 * session exchange. If what you want is to *ask* the user for a credential,
 * declare it in your manifest's `setup.credentials` and moss will draw the
 * field; a plugin-drawn password box is a registry blocker, not a style choice.
 *
 * Refused for any key your manifest declared as one moss asks the user for — a
 * `setup.credentials` entry, or a `config_schema` field of type `secret`. Those
 * slots hold what a person typed into moss's modal, and a plugin quietly
 * replacing one would leave the user believing their own token is still there.
 * If you need such a credential replaced, call {@link rejectSecret}: moss
 * forgets it and asks again, and you get the new value back. Every other key in
 * your scope is yours to write. The key is scoped to your plugin automatically
 * — you cannot write another plugin's secret, the same way you cannot read one.
 *
 * An empty `value` erases the key — that is how you sign a user out, and moss
 * then reports nothing stored for the slot. Earlier releases stored the empty
 * string literally, so a signed-out account went on showing as connected.
 *
 * Why here and not in your own plugin folder: `.moss/plugins/` is inside the
 * user's repo and is not gitignored, so a token you keep yourself is a token
 * that gets committed and pushed. This is the same custody argument moss already
 * makes for signing keys.
 *
 * @category Secrets
 */
export async function setSecret(key: string, value: string): Promise<void> {
  await getTauriCore().invoke<null>("set_plugin_secret", { key, value });
}
