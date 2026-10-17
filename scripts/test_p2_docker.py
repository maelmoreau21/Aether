"""Real disposable Docker integration. Requires Windows Docker Engine pipe for secure TTY bootstrap.

No existing containers/volumes are addressed. Secrets and certificates stay in target/.
Run: python scripts/test_p2_docker.py [--baseline]
"""
import concurrent.futures
import contextlib
import hashlib
import http.client
import io
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import threading
import time
import uuid
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
BASELINE = "--baseline" in sys.argv
PREFIX = "p2qa-" + uuid.uuid4().hex[:10]
WORK = ROOT / "target" / PREFIX
WORK.mkdir(parents=True)
REPORT = {"run": PREFIX, "baseline": BASELINE, "transport": "trusted HTTP bridge" if BASELINE else "HTTPS via test TLS terminator", "phases": {}, "checks": []}
CONTAINERS, VOLUMES = [], []
PASSWORD = secrets.token_hex(24)
TOKENS = [secrets.token_hex(32), secrets.token_hex(32)]
LOG_SECRETS = [PASSWORD, *TOKENS]
ENV = dict(os.environ)
NETWORK = PREFIX + "-net"
FIXTURE_HASHES = set()


def docker(*args, input=None):
    result = subprocess.run(["docker", *map(str, args)], input=input, capture_output=True, text=True, timeout=300)
    if result.returncode:
        raise RuntimeError(f"docker {args[0]} failed (details suppressed to avoid secret disclosure)")
    return (result.stdout + (result.stderr if args[0] == "logs" else "")).strip()


def request(origin, method, path, body=None, headers=None):
    url = urlsplit(origin)
    connection = http.client.HTTPConnection(url.hostname, url.port, timeout=30)
    all_headers = {"Origin": origin, **(headers or {})}
    if body is not None:
        body = json.dumps(body)
        all_headers["Content-Type"] = "application/json"
    started = time.perf_counter()
    connection.request(method, path, body, all_headers)
    response = connection.getresponse()
    first = time.perf_counter()
    data = response.read()
    result = response.status, dict(response.getheaders()), data, (first - started) * 1000
    connection.close()
    return result


def check(value, label):
    if not value:
        raise AssertionError(label)
    REPORT["checks"].append(label)


def wait(origin, path="/api/v1/health"):
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        try:
            if request(origin, "GET", path)[0] == 200:
                return
        except OSError:
            pass
        time.sleep(.3)
    raise TimeoutError("container readiness")


def bootstrap(name):
    # Drive the real local TTY CLI; wait for prompts, never echo credentials.
    import re
    with open(r"\\.\pipe\docker_engine", "r+b", buffering=0) as pipe:
        payload = json.dumps({"AttachStdin": True, "AttachStdout": True, "AttachStderr": True, "Tty": True, "Cmd": ["media-bootstrap"]}).encode()
        pipe.write((f"POST /containers/{name}/exec HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len(payload)}\r\n\r\n").encode() + payload)
        data = b""
        while not re.search(rb'\{"Id":"[a-f0-9]+"\}', data):
            data += pipe.read(512)
        exec_id = json.loads(re.search(rb'\{"Id":"[a-f0-9]+"\}', data).group())["Id"]
    with open(r"\\.\pipe\docker_engine", "r+b", buffering=0) as pipe:
        payload = b'{"Detach":false,"Tty":true}'
        pipe.write((f"POST /exec/{exec_id}/start HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nConnection: Upgrade\r\nUpgrade: tcp\r\nContent-Length: {len(payload)}\r\n\r\n").encode() + payload)
        def until(marker):
            data = b""
            while marker not in data:
                chunk = pipe.read(1)
                if not chunk: raise AssertionError("bootstrap closed before prompt")
                data += chunk
            return data
        until(b"Login (lowercase): ")
        pipe.write(b"qa-admin\n")
        until(b"Password: ")
        pipe.write((PASSWORD + "\n").encode())
        until(b"Confirm password: ")
        pipe.write((PASSWORD + "\n").encode())
        until(b"Administrator created.")


def login(origin):
    status, headers, body, _ = request(origin, "POST", "/api/v1/auth/login", {"login": "qa-admin", "password": PASSWORD})
    check(status == 200, "login " + origin)
    result = {"Cookie": headers["set-cookie"].split(";")[0], "x-csrf-token": json.loads(body)["csrfToken"]}
    LOG_SECRETS.extend([result["Cookie"], result["Cookie"].split("=", 1)[1], result["x-csrf-token"]])
    return result


def run_container(suffix, image, port, host_port, environment, media=None):
    name = PREFIX + "-" + suffix
    volume = name + "-data"
    docker("volume", "create", volume)
    VOLUMES.append(volume)
    env_file = WORK / (suffix + ".env")
    env_file.write_text("\n".join(f"{k}={v}" for k, v in environment.items()), encoding="utf-8")
    args = ["run", "-d", "--name", name, "--network", NETWORK, "--network-alias", suffix,
            "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true", "--read-only",
            "--tmpfs", "/tmp:rw,noexec,nosuid,size=64m", "--env-file", env_file,
            "-p", f"127.0.0.1:{host_port}:{port}", "-v", f"{volume}:/data",
            "-v", f"{WORK.as_posix()}:/certs:ro"]
    if media:
        args += ["-v", f"{media.as_posix()}:/media:ro"]
    docker(*args, image)
    CONTAINERS.append(name)
    return name


def counters(name):
    output = docker("exec", name, "sh", "-c", "cat /sys/fs/cgroup/memory.current /sys/fs/cgroup/memory.peak /sys/fs/cgroup/cpu.stat; cat /proc/net/dev")
    lines = output.splitlines()
    cpu = next(int(line.split()[1]) for line in lines if line.startswith("usage_usec "))
    net = [line.split(":", 1)[1].split() for line in lines if ":" in line and "lo:" not in line]
    return {"memory_current": int(lines[0]), "memory_peak_since_start": int(lines[1]), "cpu_usec": cpu,
            "rx_bytes": sum(int(v[0]) for v in net), "tx_bytes": sum(int(v[8]) for v in net)}


@contextlib.contextmanager
def measured(label, names):
    before = {n: counters(n) for n in names}
    samples = {n: [] for n in names}
    stop = threading.Event()
    def sample():
        while not stop.is_set():
            for n in names:
                samples[n].append(counters(n)["memory_current"])
            stop.wait(.2)
    worker = threading.Thread(target=sample)
    worker.start()
    started = time.perf_counter()
    try:
        yield
    finally:
        elapsed = time.perf_counter() - started
        stop.set()
        worker.join()
        after = {n: counters(n) for n in names}
        REPORT["phases"][label] = {"seconds": elapsed, "containers": {
            n.removeprefix(PREFIX + "-"): {**after[n], "sampled_peak_bytes": max(samples[n] or [after[n]["memory_current"]]),
                "cpu_seconds": (after[n]["cpu_usec"] - before[n]["cpu_usec"]) / 1e6,
                "cpu_one_core_percent": (after[n]["cpu_usec"] - before[n]["cpu_usec"]) / 1e4 / elapsed,
                "rx_delta": after[n]["rx_bytes"] - before[n]["rx_bytes"], "tx_delta": after[n]["tx_bytes"] - before[n]["tx_bytes"], "rx_bytes_per_second": (after[n]["rx_bytes"] - before[n]["rx_bytes"])/elapsed, "tx_bytes_per_second": (after[n]["tx_bytes"] - before[n]["tx_bytes"])/elapsed}
            for n in names}}


def scan(origin, auth, library):
    status, _, _, _ = request(origin, "POST", f"/api/v1/admin/libraries/{library}/scan", {}, auth)
    check(status == 202, f"scan accepted ({status})")
    scan_start = time.perf_counter()
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        status, _, body, _ = request(origin, "GET", f"/api/v1/admin/libraries/{library}/scan", headers=auth)
        value = json.loads(body)
        job = value.get("job", value)
        if job.get("status") in ("completed", "complete", "success"):
            check(job.get("errorsCount", 0) == 0, "scan without media errors")
            REPORT.setdefault("scans", []).append({"seconds": time.perf_counter() - scan_start, "discovered": job.get("discovered"), "probed": job.get("probed"), "reused": job.get("reused")})
            return
        if job.get("status") in ("error", "failed"):
            raise AssertionError("scan failed")
        time.sleep(.1)
    raise TimeoutError("scan completion")


def assets(origin, auth):
    status, _, body, latency = request(origin, "GET", "/api/v1/catalog?kind=film", headers=auth)
    REPORT.setdefault("api_ttfb_ms", []).append(latency)
    check(status == 200, "catalog available")
    # Walk the real catalogue API to find both film and episode assets.
    found = []
    for kind in ["film", "series"]:
        _, _, data, _ = request(origin, "GET", "/api/v1/catalog?kind=" + kind, headers=auth)
        catalog = json.loads(data)
        for item in catalog.get("items", []):
            _, _, data, _ = request(origin, "GET", "/api/v1/catalog/" + item["id"], headers=auth)
            def visit(value):
                if isinstance(value, dict):
                    if value.get("playbackMediaId"):
                        found.append(value["playbackMediaId"])
                    for v in value.values(): visit(v)
                elif isinstance(value, list):
                    for v in value: visit(v)
            visit(json.loads(data))
    return list(dict.fromkeys(found))


def stream(origin, auth, ids, distributed, route_mode="both"):
    direct_urls = []
    for asset in ids:
        status, _, data, _ = request(origin, "GET", "/api/v1/media/" + asset, headers=auth)
        check(status == 200, "descriptor available")
        source = json.loads(data)["sources"][0]
        routes = [(origin, source.get("relayUrl", source["url"]), auth)] if route_mode != "direct" else []
        if distributed:
            u = urlsplit(source["directUrl"])
            direct_urls.append(source["directUrl"])
            if route_mode != "relay":
                routes.append((f"{u.scheme}://{u.netloc}", u.path + "?" + u.query, {}))
        for base, route, headers in routes:
            status, _, full, latency = request(base, "GET", route, headers=headers)
            check(status == 200 and len(full) > 100, "full stream")
            check(hashlib.sha256(full).hexdigest() in FIXTURE_HASHES, "stream matches original fixture hash")
            REPORT.setdefault("stream_ttfb_ms", []).append(latency)
            check(request(base, "HEAD", route, headers=headers)[0] == 200, "HEAD")
            status, h, part, _ = request(base, "GET", route, headers={**headers, "Range": "bytes=0-127"})
            check(status == 206 and part == full[:128], "Range exact bytes")
            check(request(base, "GET", route, headers={**headers, "Range": "bytes=999999999999-"})[0] == 416, "invalid Range")
            # Abort a response before consuming its body, then verify the next request.
            c = http.client.HTTPConnection(urlsplit(base).hostname, urlsplit(base).port, timeout=20)
            c.request("GET", route, headers=headers)
            r = c.getresponse(); r.read(1); c.close()
            check(request(base, "HEAD", route, headers=headers)[0] == 200, "request after cancellation")
    return direct_urls


def read_load(origin, auth, ids, route_mode, seconds=5):
    routes = []
    for asset in ids:
        _, _, data, _ = request(origin, "GET", "/api/v1/media/" + asset, headers=auth)
        source = json.loads(data)["sources"][0]
        if route_mode == "direct":
            u = urlsplit(source["directUrl"])
            routes.append((f"{u.scheme}://{u.netloc}", u.path + "?" + u.query, {}))
        else:
            routes.append((origin, source.get("relayUrl", source["url"]), auth))
    started = time.perf_counter()
    latencies, transferred = [], 0
    while time.perf_counter() - started < seconds:
        for base, path, headers in routes:
            status, _, data, latency = request(base, "GET", path, headers=headers)
            if status != 200:
                raise AssertionError("read load failed")
            transferred += len(data)
            latencies.append(latency)
    latencies.sort()
    REPORT.setdefault("read_load", []).append({"mode": route_mode, "seconds": time.perf_counter() - started,
        "requests": len(latencies), "payload_bytes": transferred,
        "ttfb_p50_ms": latencies[len(latencies)//2], "ttfb_p95_ms": latencies[min(len(latencies)-1, int(len(latencies)*.95))]})


def main():
    import datetime
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import rsa
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    now = datetime.datetime.now(datetime.timezone.utc)
    name = x509.Name([x509.NameAttribute(x509.NameOID.COMMON_NAME, "Disposable P2 QA CA")])
    cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(key.public_key()).serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(minutes=5)).not_valid_after(now + datetime.timedelta(days=1))
            .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
            .add_extension(x509.SubjectAlternativeName([x509.DNSName(n) for n in ["tls-core", "tls-films", "tls-series"]]), critical=False)
            .sign(key, hashes.SHA256()))
    (WORK / "server.pem").write_bytes(cert.public_bytes(serialization.Encoding.PEM))
    (WORK / "server.key").write_bytes(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    media = WORK / "media"
    for kind in ["films", "series"]:
        for i in range(12):
            folder = media / kind / (f"Fixture {i:02} (2025)" if kind == "films" else "Fixture Series (2025)/Season 01")
            folder.mkdir(parents=True, exist_ok=True)
            for ext in ["mp4", "mkv"]:
                filename = f"Fixture {i:02}.{ext}" if kind == "films" else f"Fixture Series S01E{i+1:02}.{ext}"
                shutil.copyfile(ROOT / "tests/fixtures/generated" / ("av1-opus." + ext), folder / filename)
    original = {str(p.relative_to(media)): hashlib.sha256(p.read_bytes()).hexdigest() for p in media.rglob("*") if p.is_file()}
    FIXTURE_HASHES.update(original.values())
    docker("network", "create", NETWORK)
    node_image = "aether-node:latest" if BASELINE else "aether-node:p2-qa"
    core_image = "aether-core:latest" if BASELINE else "aether-core:p2-qa"
    standalone_image = "aether:test" if BASELINE else "aether:p2-qa"
    REPORT["image_bytes"] = {image: int(docker("image", "inspect", image, "--format", "{{.Size}}")) for image in [node_image, core_image, standalone_image]}
    REPORT["image_ids"] = {image: docker("image", "inspect", image, "--format", "{{.Id}}") for image in [node_image, core_image, standalone_image]}
    REPORT["docker_server"] = docker("version", "--format", "{{.Server.Version}}")
    authority = "core:3000,films:4101,series:4102" if BASELINE else "tls-core:4300,tls-films:4301,tls-series:4302,tls-wrong:4301"
    common = {"MEDIA_INTERNAL_ALLOWED_AUTHORITIES": authority, "MEDIA_INTERNAL_TRUSTED_HTTP": "true" if BASELINE else "false", "MEDIA_INTERNAL_CA_FILE": "/certs/server.pem"}
    origin = "http://127.0.0.1:13000"
    core = run_container("core", core_image, 3000, 13000, {**common, "MEDIA_MODE": "distributed", "MEDIA_PUBLIC_URL": origin,
        "MEDIA_CORE_INTERNAL_URL": "http://core:3000" if BASELINE else "https://tls-core:4300"})
    nodes = []
    for i, kind in enumerate(["films", "series"]):
        nodes.append(run_container(kind, node_image, 4101 + i, 14101 + i, {**common, "MEDIA_NODE_PORT": str(4101 + i),
            "MEDIA_NODE_ROOTS": "/media/" + kind, "MEDIA_NODE_PAIRING_TOKEN": TOKENS[i], "MEDIA_NODE_PUBLIC_URL": f"http://127.0.0.1:{14101+i}"}, media))
    if not BASELINE:
        proxy = PREFIX + "-tls"
        docker("run", "-d", "--name", proxy, "--network", NETWORK, "--network-alias", "tls-core", "--network-alias", "tls-films", "--network-alias", "tls-series", "--network-alias", "tls-wrong",
            "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true", "--read-only", "--tmpfs", "/tmp", "-v", f"{WORK.as_posix()}:/certs:ro",
            "-v", f"{(ROOT/'scripts/p2_tls_proxy.py').as_posix()}:/proxy.py:ro", "python:3.13-alpine", "python", "/proxy.py")
        CONTAINERS.append(proxy)
    wait(origin)
    bootstrap(core)
    auth = login(origin)
    if not BASELINE:
        status, _, _, _ = request(origin, "POST", "/api/v1/admin/nodes/pair", {"name": "wrong hostname", "baseUrl": "https://tls-wrong:4301", "pairingToken": TOKENS[0]}, auth)
        check(status == 502, "Docker HTTPS hostname mismatch rejected")
    node_ids, libs = [], []
    for i, kind in enumerate(["films", "series"]):
        status, _, data, _ = request(origin, "POST", "/api/v1/admin/nodes/pair", {"name": "QA " + kind,
            "baseUrl": f"http://{kind}:{4101+i}" if BASELINE else f"https://tls-{kind}:{4301+i}",
            "publicUrl": f"http://127.0.0.1:{14101+i}", "pairingToken": TOKENS[i]}, auth)
        check(status == 201, "pair " + kind)
        node_id = json.loads(data)["id"]; node_ids.append(node_id)
        status, _, data, _ = request(origin, "POST", "/api/v1/admin/libraries", {"name": "QA " + kind, "kind": kind, "nodeId": node_id, "root": "/media/" + kind, "enabled": True, "userIds": []}, auth)
        check(status == 201, "create " + kind)
        libs.append(json.loads(data)["id"])
    measured_nodes = [core, *nodes] + ([proxy] if not BASELINE else [])
    with measured("distributed_scan", measured_nodes):
        for lib in libs: scan(origin, auth, lib)
    ids = assets(origin, auth)
    check(len(ids) >= 2, "film and episode indexed")
    with measured("distributed_direct", measured_nodes):
        direct = stream(origin, auth, [ids[0], ids[-1]], True, "direct")
        read_load(origin, auth, [ids[0], ids[-1]], "direct")
    with measured("distributed_relay", measured_nodes):
        stream(origin, auth, [ids[0], ids[-1]], True, "relay")
        read_load(origin, auth, [ids[0], ids[-1]], "relay")
    if not BASELINE:
        browser_env = {**os.environ, "MEDIA_TEST_ORIGIN": origin, "MEDIA_TEST_ADMIN_PASS": PASSWORD, "MEDIA_REPORT_DIR": str(WORK / "browser-distributed")}
        subprocess.run(["node", "scripts/browser-p2-validation.cjs"], env=browser_env, check=True, timeout=180)
    # Persistence, actual stop/reconnection, secret rotation, logout revocation.
    docker("restart", core, *nodes); wait(origin)
    auth = login(origin)
    check(set(assets(origin, auth)) == set(ids), "distributed restart persists IDs")
    docker("stop", nodes[0])
    status, _, data, _ = request(origin, "POST", f"/api/v1/admin/nodes/{node_ids[0]}/reconnect", {}, auth)
    check(status == 200 and json.loads(data)["status"] == "disconnected", "stopped Node disconnected")
    docker("start", nodes[0]); wait("http://127.0.0.1:14101", "/internal/v1/health")
    check(request(origin, "POST", f"/api/v1/admin/nodes/{node_ids[0]}/reconnect", {}, auth)[0] == 200, "Node reconnect")
    check(request(origin, "POST", f"/api/v1/admin/nodes/{node_ids[0]}/rotate-secret", {}, auth)[0] == 200, "secret rotation")
    docker("restart", nodes[0]); wait("http://127.0.0.1:14101", "/internal/v1/health")
    u = urlsplit(direct[0]); check(request(f"{u.scheme}://{u.netloc}", "HEAD", u.path + "?" + u.query)[0] == 403, "old token after rotation")
    if not BASELINE:
        # Issue a fresh token in this login, log out, then retry it directly.
        _, _, data, _ = request(origin, "GET", "/api/v1/media/" + ids[0], headers=auth)
        fresh = urlsplit(json.loads(data)["sources"][0]["directUrl"])
        check(request(f"{fresh.scheme}://{fresh.netloc}", "HEAD", fresh.path + "?" + fresh.query)[0] == 200, "rotated identity persists across Node restart")
        docker("stop", core)
        check(request(f"{fresh.scheme}://{fresh.netloc}", "HEAD", fresh.path + "?" + fresh.query)[0] == 403, "Core outage fails closed for direct playback")
        docker("start", core); wait(origin)
        check(request(f"{fresh.scheme}://{fresh.netloc}", "HEAD", fresh.path + "?" + fresh.query)[0] == 200, "direct playback recovers after Core restart")
        check(request(origin, "POST", "/api/v1/auth/logout", {}, auth)[0] == 204, "logout")
        check(request(f"{fresh.scheme}://{fresh.netloc}", "HEAD", fresh.path + "?" + fresh.query)[0] == 403, "old token after logout")
        auth = login(origin)
    docker("stop", nodes[1])
    check(request(origin, "POST", f"/api/v1/admin/nodes/{node_ids[1]}/revoke", {}, auth)[0] == 200, "offline Node revoke")
    docker("start", nodes[1]); wait("http://127.0.0.1:14102", "/internal/v1/health")
    if not BASELINE:
        u = urlsplit(direct[-1]); check(request(f"{u.scheme}://{u.netloc}", "GET", u.path + "?" + u.query)[0] == 403, "offline revoke denies old token after restart")
    standalone_origin = "http://127.0.0.1:13001"
    single = run_container("standalone", standalone_image, 3000, 13001, {"MEDIA_PUBLIC_URL": standalone_origin}, media)
    wait(standalone_origin); bootstrap(single); standalone_auth = login(standalone_origin)
    standalone_libs = []
    for kind in ["films", "series"]:
        status, _, data, _ = request(standalone_origin, "POST", "/api/v1/admin/libraries", {"name": "Standalone " + kind, "kind": kind, "nodeId": "local", "root": "/media/" + kind, "enabled": True, "userIds": []}, standalone_auth)
        check(status == 201, "standalone create " + kind); standalone_libs.append(json.loads(data)["id"])
    with measured("standalone_scan", [single]):
        for lib in standalone_libs: scan(standalone_origin, standalone_auth, lib)
    single_ids = assets(standalone_origin, standalone_auth)
    with measured("standalone_stream", [single]):
        stream(standalone_origin, standalone_auth, [single_ids[0], single_ids[-1]], False)
        read_load(standalone_origin, standalone_auth, [single_ids[0], single_ids[-1]], "relay")
    if not BASELINE:
        browser_env = {**os.environ, "MEDIA_TEST_ORIGIN": standalone_origin, "MEDIA_TEST_ADMIN_PASS": PASSWORD, "MEDIA_REPORT_DIR": str(WORK / "browser-standalone")}
        subprocess.run(["node", "scripts/browser-p2-validation.cjs"], env=browser_env, check=True, timeout=180)
    docker("restart", single); wait(standalone_origin); standalone_auth = login(standalone_origin)
    check(set(assets(standalone_origin, standalone_auth)) == set(single_ids), "standalone restart persists IDs")
    check(original == {str(p.relative_to(media)): hashlib.sha256(p.read_bytes()).hexdigest() for p in media.rglob("*") if p.is_file()}, "all media unchanged")
    # Save only non-secret browser configuration. Credentials passed via child environment.
    (WORK / "browser.json").write_text(json.dumps({"origin": origin, "ids": [ids[0], ids[-1]], "standaloneOrigin": standalone_origin, "standaloneIds": [single_ids[0], single_ids[-1]]}))
    for name in CONTAINERS:
        logs = docker("logs", name)
        check(not any(value in logs for value in LOG_SECRETS) and "?token=" not in logs, "container logs contain no credentials or playback query")
    REPORT["success"] = True


if __name__ == "__main__":
    try:
        main()
    finally:
        REPORT["work_directory"] = str(WORK)
        (WORK / "report.json").write_text(json.dumps(REPORT, indent=2), encoding="utf-8")
        print(json.dumps({"report": str(WORK / "report.json"), "success": REPORT.get("success", False)}))
        # Leave failed runs for inspection; cleanup only this run's explicitly recorded names.
        if REPORT.get("success"):
            for name in reversed(CONTAINERS): docker("rm", "-f", "-v", name)
            for name in VOLUMES: docker("volume", "rm", name)
            docker("network", "rm", NETWORK)
