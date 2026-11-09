# Editing in a browser

Read `moss guide` (the overview) first: it holds the task recipes this reference does not repeat.

`moss edit <folder> --serve` builds the folder, watches it, and serves the editor and the preview to a browser. On the machine that holds the folder it opens a one-time sign-in link in the default browser; with `--no-browser`, or when stdin is not a terminal, it prints the link and a QR code instead. A second `moss edit` on the same folder asks the running one for a fresh link. `moss sessions` lists the browsers signed in, and `moss sessions revoke <id>` (or `--all`) signs one out.

## From another device, or on a server

moss listens on loopback only. To edit from a phone or another computer, or to run moss on a server, put it behind an HTTPS reverse proxy you run (Tailscale Serve, Caddy, nginx and the like) and tell moss the origin the proxy serves the editor at:

    moss edit <folder> --serve --no-browser --port 8443 --public-url https://edit.example.com

The editor and the preview are two origins, because the preview runs the site's own scripts and must not share the editor's. The proxy serves the preview at the same host on the next port (`https://edit.example.com:444` above), or on the port `--public-preview-port` names. moss prints where the proxy sends each:

    Proxy https://edit.example.com to http://127.0.0.1:8443
    Proxy https://edit.example.com:444 to http://127.0.0.1:8444

Set the proxy up for both. It may pass the public host on as `Host` or rewrite it to the `127.0.0.1` address it forwards to (nginx's default); moss answers both, and asks for a sign-in either way. A `Host` naming neither, such as an upstream's name, is refused with 421. Edits reach the open page over a long-lived event stream; moss tells nginx not to buffer it, and a proxy that buffers responses some other way needs buffering turned off for both origins. A proxy that serves only some ports needs `--public-preview-port`: Tailscale Funnel serves 443, 8443 and 10000, so `--public-url https://host.example.ts.net --public-preview-port 8443` puts the preview on one it serves. Under `--public-url` both local ports are fixed, and a port already taken is an error that names it, never a move to the next one; `--public-url` needs `--port` for that reason.

Then open the printed link on the other device, or click "Open on another device" in a browser already signed in and scan its QR code. With a public URL set, every page asks for a signed-in browser, the preview and unpublished pages included, from every device; only the readiness check at `/__moss_health/` answers without one. Without `--public-url`, nothing is reachable from another machine.
