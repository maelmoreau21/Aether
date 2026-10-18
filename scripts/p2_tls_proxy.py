"""Disposable integration-only TLS terminator. Never logs URLs or headers."""
import http.client
import http.server
import ssl
import threading


def serve(port, host, upstream):
    class Proxy(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_):
            pass

        def forward(self):
            connection = http.client.HTTPConnection(host, upstream, timeout=30)
            try:
                body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
                headers = {k: v for k, v in self.headers.items() if k.lower() not in ("connection", "host")}
                headers["Host"] = f"{host}:{upstream}"
                connection.request(self.command, self.path, body, headers)
                response = connection.getresponse()
                self.send_response(response.status)
                for key, value in response.getheaders():
                    if key.lower() not in ("connection", "transfer-encoding"):
                        self.send_header(key, value)
                self.send_header("Connection", "close")
                self.end_headers()
                if self.command != "HEAD":
                    while chunk := response.read(65536):
                        self.wfile.write(chunk)
                self.close_connection = True
            except (BrokenPipeError, ConnectionResetError):
                self.close_connection = True
            finally:
                connection.close()

        do_GET = do_HEAD = do_POST = forward

    server = http.server.ThreadingHTTPServer(("0.0.0.0", port), Proxy)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain("/certs/server.pem", "/certs/server.key")
    server.socket = context.wrap_socket(server.socket, server_side=True)
    server.serve_forever()


if __name__ == "__main__":
    for args in [(4300, "core", 3000), (4301, "films", 4101), (4302, "series", 4102)]:
        threading.Thread(target=serve, args=args, daemon=True).start()
    threading.Event().wait()
