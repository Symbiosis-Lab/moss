#!/usr/bin/env python3
"""Regenerates the `moss desktop install` test fixtures in `desktop_tests.rs`.

Builds the one clean tarball fixture the test decodes as a base64 constant
(carrying the allowed contained-relative symlink case), and signs it with a
throwaway minisign-format Ed25519 keypair (generated fresh on every run via
`openssl genpkey`; the secret half never leaves a temp directory this script
creates and removes, and is never checked in anywhere — only its signature
output is). Prints Rust `const ..._B64: &str = "...";` lines ready to paste
over the matching constants in `crates/moss-cli/src/desktop_tests.rs`.

The hostile shapes (traversal, absolute path, hardlink, symlink escaping the
target) are no longer built here: `install_bundle` unpacks through
`moss_build::system::tar_safe::extract_tar_gz`, so those refusals are tested
once, in `crates/moss-build/src/system/tar_safe_tests.rs`.

Requires: python3 (stdlib only) and the `openssl` CLI (ed25519 support,
OpenSSL >= 1.1.1). No network access, no non-stdlib Python packages.

Usage: python3 gen_desktop_fixtures.py
"""
import base64
import hashlib
import io
import os
import subprocess
import tarfile
import tempfile
import time


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def add_dir(tf: tarfile.TarFile, name: str):
    ti = tarfile.TarInfo(name=name)
    ti.type = tarfile.DIRTYPE
    ti.mode = 0o755
    tf.addfile(ti)


def add_file(tf: tarfile.TarFile, name: str, content: bytes):
    ti = tarfile.TarInfo(name=name)
    ti.size = len(content)
    ti.mode = 0o644
    tf.addfile(ti, io.BytesIO(content))


def add_symlink(tf: tarfile.TarFile, name: str, target: str):
    ti = tarfile.TarInfo(name=name)
    ti.type = tarfile.SYMTYPE
    ti.linkname = target
    ti.mode = 0o644
    tf.addfile(ti)


def build_tar_gz(builder) -> bytes:
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tf:
        builder(tf)
    return buf.getvalue()


INFO_PLIST = b'<?xml version="1.0"?><plist/>'


def build_clean_with_symlink(tf: tarfile.TarFile):
    add_dir(tf, "moss.app")
    add_dir(tf, "moss.app/Contents")
    add_file(tf, "moss.app/Contents/Info.plist", INFO_PLIST)
    add_dir(tf, "moss.app/Contents/Frameworks")
    add_dir(tf, "moss.app/Contents/Frameworks/X.framework")
    add_dir(tf, "moss.app/Contents/Frameworks/X.framework/Versions")
    add_dir(tf, "moss.app/Contents/Frameworks/X.framework/Versions/A")
    # A relative symlink that resolves to .../Versions/A, staying inside the
    # extraction root — the ALLOWED case `extract_into`'s guard must accept,
    # and the one reason this extractor differs from a plain traversal guard.
    add_symlink(
        tf,
        "moss.app/Contents/Frameworks/X.framework/Versions/Current",
        "A",
    )


# --- minisign signing (throwaway keypair, generated fresh each run) -------


def gen_ed25519_keypair(workdir: str):
    priv_path = os.path.join(workdir, "throwaway_ed25519.pem")
    subprocess.run(
        ["openssl", "genpkey", "-algorithm", "ed25519", "-out", priv_path],
        check=True,
        capture_output=True,
    )
    pub_der = subprocess.run(
        ["openssl", "pkey", "-in", priv_path, "-pubout", "-outform", "DER"],
        check=True,
        capture_output=True,
    ).stdout
    # An Ed25519 SubjectPublicKeyInfo DER is a fixed 12-byte prefix followed
    # by the 32-byte raw public key.
    raw_pub = pub_der[-32:]
    return priv_path, raw_pub


def raw_ed25519_sign(priv_path: str, message: bytes, workdir: str) -> bytes:
    msg_path = os.path.join(workdir, f"msg-{time.time_ns()}.bin")
    sig_path = msg_path + ".sig"
    with open(msg_path, "wb") as f:
        f.write(message)
    subprocess.run(
        [
            "openssl", "pkeyutl", "-sign", "-inkey", priv_path,
            "-rawin", "-in", msg_path, "-out", sig_path,
        ],
        check=True,
        capture_output=True,
    )
    with open(sig_path, "rb") as f:
        return f.read()


def minisign_pubkey_file(raw_pub: bytes, key_id: bytes) -> str:
    blob = b"Ed" + key_id + raw_pub
    key_id_hex = key_id[::-1].hex().upper()
    return f"untrusted comment: minisign public key {key_id_hex}\n{b64(blob)}\n"


def minisign_sign_file(priv_path: str, key_id: bytes, message: bytes, filename: str, workdir: str) -> str:
    digest = hashlib.blake2b(message, digest_size=64).digest()
    sig = raw_ed25519_sign(priv_path, digest, workdir)
    sig_blob = b"ED" + key_id + sig
    trusted_comment_body = f"timestamp:{int(time.time())}\tfile:{filename}"
    trusted_comment = f"trusted comment: {trusted_comment_body}"
    # The global signature covers `sig` plus the trusted comment's CONTENT,
    # not the "trusted comment: " prefix — `Signature::trusted_comment()` in
    # minisign-verify strips exactly those 17 bytes before checking this.
    global_sig = raw_ed25519_sign(priv_path, sig + trusted_comment_body.encode(), workdir)
    return (
        "untrusted comment: signature from minisign secret key\n"
        f"{b64(sig_blob)}\n"
        f"{trusted_comment}\n"
        f"{b64(global_sig)}\n"
    )


def main():
    with tempfile.TemporaryDirectory() as workdir:
        priv_path, raw_pub = gen_ed25519_keypair(workdir)
        key_id = os.urandom(8)

        clean = build_tar_gz(build_clean_with_symlink)

        pubkey_file = minisign_pubkey_file(raw_pub, key_id)
        sig_file = minisign_sign_file(priv_path, key_id, clean, "moss.app.tar.gz", workdir)

        print(f'const CLEAN_TARBALL_B64: &str = "{b64(clean)}";')
        print(f'const FIXTURE_PUBKEY_B64: &str = "{b64(pubkey_file.encode())}";')
        print(f'const FIXTURE_SIG_B64: &str = "{b64(sig_file.encode())}";')


if __name__ == "__main__":
    main()
