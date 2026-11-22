"""The render gates' static server: `python3 -m http.server <port>` from the current folder.

Its listen backlog is 128, not socketserver's 5: a page requests its scripts at once,
and under load the sixth connection was dropped, so that page ran without its script.
With GATE_INPUTS_OUT set it appends each file it serves, as tests/support/gate-inputs.cjs
does for Node, so what the browser loads through it is part of the gate's inputs.
It exits when the process that started it is gone: a server orphaned by a killed
Playwright would hold its gate port, and the next run on that port fails to start.
"""
import contextlib
import http.server
import os
import socket
import sys
import threading
import time

OUT = os.environ.get("GATE_INPUTS_OUT")


class Handler(http.server.SimpleHTTPRequestHandler):
    def translate_path(self, path):
        served = super().translate_path(path)
        if OUT:
            # A folder is served as its index.html.
            read = os.path.join(served, "index.html") if os.path.isdir(served) else served
            with open(OUT, "a") as log:
                log.write(f"read\t{read}\n")
        return served


class Server(http.server.ThreadingHTTPServer):
    request_queue_size = 128

    def server_bind(self):
        # Both stacks, as `-m http.server` binds: localhost may resolve to ::1.
        with contextlib.suppress(Exception):
            self.socket.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0)
        return super().server_bind()


def exit_with_parent(parent):
    while os.getppid() == parent:
        time.sleep(1)
    os._exit(0)


threading.Thread(target=exit_with_parent, args=(os.getppid(),), daemon=True).start()
http.server.test(HandlerClass=Handler, ServerClass=Server, port=int(sys.argv[1]))
