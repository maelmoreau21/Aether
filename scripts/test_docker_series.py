import os
import sys
import json
import time
import shutil
import urllib.request
import urllib.error
import http.client
import subprocess
import re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
TEST_DIR = os.path.join(ROOT, "target", "docker_series_test")
DATA_DIR = os.path.join(TEST_DIR, "data")
MEDIA_DIR = os.path.join(TEST_DIR, "media")
SERIES_DIR = os.path.join(MEDIA_DIR, "series", "Centennial (2025)")

CONTAINER_NAME = "aether-docker-test"
PORT = 3001
ADMIN_USER = "admin"
ADMIN_PASS = "AetherDocker2026!"

def clean_docker():
    print("[docker] Cleaning up any old container...")
    subprocess.run(["docker", "rm", "-f", CONTAINER_NAME], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def setup_fixtures():
    print("[setup] Preparing series fixture files...")
    if os.path.exists(TEST_DIR):
        shutil.rmtree(TEST_DIR)
    os.makedirs(DATA_DIR, exist_ok=True)
    os.makedirs(os.path.join(SERIES_DIR, "Season 01"), exist_ok=True)
    os.makedirs(os.path.join(SERIES_DIR, "Season 02"), exist_ok=True)

    sample_mp4 = os.path.join(ROOT, "tests", "fixtures", "generated", "av1-opus.mp4")
    sample_mkv = os.path.join(ROOT, "tests", "fixtures", "generated", "av1-opus.mkv")

    assert os.path.exists(sample_mp4), f"Missing {sample_mp4}"
    assert os.path.exists(sample_mkv), f"Missing {sample_mkv}"

    # tvshow.nfo
    with open(os.path.join(SERIES_DIR, "tvshow.nfo"), "w", encoding="utf-8") as f:
        f.write('<?xml version="1.0" encoding="UTF-8"?>\n<tvshow><title>Centennial</title><year>2025</year></tvshow>')

    # S01E01 (2 versions: mp4 and mkv)
    shutil.copyfile(sample_mp4, os.path.join(SERIES_DIR, "Season 01", "Centennial - S01E01.mp4"))
    shutil.copyfile(sample_mkv, os.path.join(SERIES_DIR, "Season 01", "Centennial - S01E01 - Version 2.mkv"))
    # S01E02
    shutil.copyfile(sample_mp4, os.path.join(SERIES_DIR, "Season 01", "Centennial - S01E02.mp4"))
    # S02E01
    shutil.copyfile(sample_mp4, os.path.join(SERIES_DIR, "Season 02", "Centennial - S02E01.mp4"))

    print("[setup] Fixtures created successfully.")

def run_container():
    print("[docker] Starting container aether:test...")
    data_mount = f"{DATA_DIR.replace(os.sep, '/')}:/data"
    media_mount = f"{MEDIA_DIR.replace(os.sep, '/')}:/media:ro"

    cmd = [
        "docker", "run", "-d",
        "--name", CONTAINER_NAME,
        "-p", f"{PORT}:3000",
        "-e", "MEDIA_PORT=3000",
        "-e", "MEDIA_BIND_ADDR=0.0.0.0",
        "-e", "MEDIA_DB_PATH=/data/core.sqlite3",
        "-e", "MEDIA_LIBRARY_ROOT=/media",
        "-e", "MEDIA_WEB_DIST=/app/apps/web/build",
        "-e", f"MEDIA_PUBLIC_URL=http://127.0.0.1:{PORT}",
        "-v", data_mount,
        "-v", media_mount,
        "aether:test"
    ]
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0:
        raise RuntimeError(f"docker run failed: {res.stderr}")
    print(f"[docker] Container started: {res.stdout.strip()[:12]}")

def wait_for_health():
    print("[health] Waiting for /api/v1/health...")
    deadline = time.time() + 20
    url = f"http://127.0.0.1:{PORT}/api/v1/health"
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=2) as resp:
                if resp.status == 200:
                    data = json.loads(resp.read().decode())
                    print(f"[health] Server is healthy: {data}")
                    return
        except Exception:
            time.sleep(0.5)
    raise TimeoutError("Server did not become healthy in 20s")

def bootstrap_admin():
    print("[bootstrap] Bootstrapping admin via Docker Engine pipe...")
    with open(r'\\.\pipe\docker_engine', 'r+b', buffering=0) as f:
        req_body = json.dumps({
            "AttachStdin": True,
            "AttachStdout": True,
            "AttachStderr": True,
            "Tty": True,
            "Cmd": ["media-bootstrap"]
        }).encode('utf-8')
        
        http_req = (
            b"POST /containers/" + CONTAINER_NAME.encode('utf-8') + b"/exec HTTP/1.1\r\n"
            b"Host: localhost\r\n"
            b"Content-Type: application/json\r\n"
            b"Content-Length: " + str(len(req_body)).encode('utf-8') + b"\r\n\r\n" + req_body
        )
        f.write(http_req)
        
        resp = b""
        while b"{" not in resp:
            chunk = f.read(512)
            if not chunk:
                break
            resp += chunk
        
        m = re.search(rb'\{.*\}', resp)
        if not m:
            resp += f.read(512)
            m = re.search(rb'\{.*\}', resp)
        exec_id = json.loads(m.group(0).decode('utf-8'))["Id"]
        print(f"[bootstrap] Exec ID created: {exec_id}")

        f_start = open(r'\\.\pipe\docker_engine', 'r+b', buffering=0)
        start_body = json.dumps({"Detach": False, "Tty": True}).encode('utf-8')
        start_req = (
            f"POST /exec/{exec_id}/start HTTP/1.1\r\n"
            f"Host: localhost\r\n"
            f"Content-Type: application/json\r\n"
            f"Connection: Upgrade\r\n"
            f"Upgrade: tcp\r\n"
            f"Content-Length: {len(start_body)}\r\n\r\n"
        ).encode('utf-8') + start_body
        f_start.write(start_req)

        hdr = b""
        while b"\r\n\r\n" not in hdr:
            chunk = f_start.read(128)
            if not chunk:
                break
            hdr += chunk

        time.sleep(0.4)
        f_start.write(f"{ADMIN_USER}\n".encode('utf-8'))
        time.sleep(0.4)
        f_start.write(f"{ADMIN_PASS}\n".encode('utf-8'))
        time.sleep(0.4)
        f_start.write(f"{ADMIN_PASS}\n".encode('utf-8'))
        time.sleep(1.0)
        output = f_start.read(2048).decode('utf-8', errors='ignore')
        print(f"[bootstrap] Output:\n{output.strip()}")
        assert "Administrator created" in output, "Bootstrap admin failed!"

def make_request(method, path, body=None, headers=None):
    conn = http.client.HTTPConnection("127.0.0.1", PORT)
    req_headers = {
        "Host": f"127.0.0.1:{PORT}",
        "Origin": f"http://127.0.0.1:{PORT}",
    }
    if headers:
        req_headers.update(headers)
    req_body = None
    if body is not None:
        req_headers["Content-Type"] = "application/json"
        req_body = json.dumps(body).encode('utf-8')
    conn.request(method, path, body=req_body, headers=req_headers)
    resp = conn.getresponse()
    resp_body = resp.read()
    conn.close()
    return resp.status, resp.headers, resp_body

def run_tests():
    print("[auth] Logging in as admin...")
    status, headers, body = make_request("POST", "/api/v1/auth/login", {
        "login": ADMIN_USER,
        "password": ADMIN_PASS
    })
    assert status == 200, f"Login failed with status {status}: {body.decode()}"
    login_data = json.loads(body.decode())
    csrf_token = login_data["csrfToken"]
    cookie_hdr = headers.get("Set-Cookie")
    session_cookie = cookie_hdr.split(";")[0] if cookie_hdr else ""
    print(f"[auth] Successfully logged in! CSRF: {csrf_token[:8]}... Cookie: {session_cookie[:15]}...")

    auth_headers = {
        "Cookie": session_cookie,
        "x-csrf-token": csrf_token
    }

    print("[library] Creating Series library in Docker...")
    lib_payload = {
        "name": "Séries Docker",
        "kind": "series",
        "nodeId": "local",
        "root": "/media/series",
        "enabled": True,
        "userIds": []
    }
    status, _, body = make_request("POST", "/api/v1/admin/libraries", lib_payload, auth_headers)
    assert status == 201, f"Create library failed with {status}: {body.decode()}"
    lib_info = json.loads(body.decode())
    lib_id = lib_info["id"]
    print(f"[library] Series library created with ID: {lib_id}")

    print("[scan] Triggering scan for series library...")
    status, _, _ = make_request("POST", f"/api/v1/admin/libraries/{lib_id}/scan", None, auth_headers)
    assert status == 202, f"Scan trigger failed: {status}"

    print("[scan] Polling scan status...")
    deadline = time.time() + 30
    scan_completed = False
    while time.time() < deadline:
        status, _, body = make_request("GET", f"/api/v1/admin/libraries/{lib_id}/scan", None, auth_headers)
        assert status == 200, f"Poll scan failed: {status}"
        data = json.loads(body.decode())
        if data["status"] == "completed":
            print(f"[scan] Scan completed! {data}")
            assert len(data.get("errors", [])) == 0, f"Errors during scan: {data.get('errors')}"
            scan_completed = True
            break
        elif data["status"] == "error":
            raise RuntimeError(f"Scan failed: {data}")
        time.sleep(0.2)
    assert scan_completed, "Scan timed out"

    print("[catalog] Checking series in catalog...")
    status, _, body = make_request("GET", "/api/v1/catalog", None, auth_headers)
    assert status == 200, f"Catalog failed: {status}"
    catalog = json.loads(body.decode())
    items = catalog.get("items", [])
    print(f"[catalog] Catalog contains {len(items)} item(s): {catalog}")
    assert len(items) >= 1, "Catalog should contain at least 1 series"
    series_work = next((w for w in items if w["kind"] == "series"), None)
    assert series_work is not None, f"Series not found in catalog items: {items}"
    series_id = series_work["id"]
    print(f"[catalog] Found series '{series_work['title']}' with ID {series_id}")

    print("[catalog] Fetching series detail...")
    status, _, body = make_request("GET", f"/api/v1/catalog/{series_id}", None, auth_headers)
    assert status == 200, f"Series detail failed: {status}"
    series_detail = json.loads(body.decode())
    seasons = series_detail.get("seasons", [])
    assert len(seasons) == 2, f"Expected 2 seasons, got {len(seasons)}"
    print(f"[catalog] Series has {len(seasons)} seasons:")
    
    # Season 1 checks
    s1 = next(s for s in seasons if s["number"] == 1)
    assert len(s1["episodes"]) == 2, f"Expected 2 episodes in S01, got {len(s1['episodes'])}"
    s1e1 = next(e for e in s1["episodes"] if e["number"] == 1)
    ep1_editions = s1e1.get("editions", [])
    assert len(ep1_editions) >= 1, "Expected at least 1 edition for S01E01"
    ep1_versions = ep1_editions[0].get("versions", [])
    assert len(ep1_versions) == 2, f"Expected 2 versions for S01E01, got {len(ep1_versions)}"
    print(f"[catalog] S01E01 correctly has 2 versions: {[v.get('label') for v in ep1_versions]}")

    # Season 2 checks
    s2 = next(s for s in seasons if s["number"] == 2)
    assert len(s2["episodes"]) == 1, f"Expected 1 episode in S02, got {len(s2['episodes'])}"
    print(f"[catalog] S02E01 is present.")

    # Episode media descriptor test
    playback_id = ep1_versions[0]["sources"][0]["playbackMediaId"]
    print(f"[media] Fetching media descriptor for S01E01 playback ID ({playback_id})...")
    status, _, body = make_request("GET", f"/api/v1/media/{playback_id}", None, auth_headers)
    assert status == 200, f"Media descriptor failed: {status}"
    media_desc = json.loads(body.decode())
    assert media_desc.get("id") == playback_id, f"Expected id {playback_id}, got {media_desc.get('id')}"
    print(f"[media] Media descriptor validated: {media_desc}")
    asset_id = playback_id

    # Streaming test: HEAD
    print("[playback] Testing HEAD request on asset...")
    status, headers, _ = make_request("HEAD", f"/api/v1/assets/{asset_id}/content", None, auth_headers)
    assert status == 200, f"HEAD failed with status {status}"
    assert headers.get("Accept-Ranges") == "bytes", "Expected Accept-Ranges: bytes"
    print("[playback] HEAD succeeded with Accept-Ranges: bytes")

    # Streaming test: Range bytes=0-63
    print("[playback] Testing GET with Range: bytes=0-63...")
    range_headers = dict(auth_headers)
    range_headers["Range"] = "bytes=0-63"
    status, headers, body = make_request("GET", f"/api/v1/assets/{asset_id}/content", None, range_headers)
    assert status == 206, f"Range request failed with status {status}: {headers}"
    assert len(body) == 64, f"Expected 64 bytes, got {len(body)}"
    content_range = headers.get("Content-Range")
    assert content_range and content_range.startswith("bytes 0-63/"), f"Unexpected Content-Range: {content_range}"
    print(f"[playback] Range 0-63 succeeded: {content_range}")

    # Streaming test: Range bytes=1000-2000 (Seek)
    print("[playback] Testing seek GET with Range: bytes=1000-2000...")
    range_headers["Range"] = "bytes=1000-2000"
    status, headers, body = make_request("GET", f"/api/v1/assets/{asset_id}/content", None, range_headers)
    assert status == 206, f"Seek Range failed with status {status}"
    assert len(body) == 1001, f"Expected 1001 bytes, got {len(body)}"
    content_range = headers.get("Content-Range")
    assert content_range and content_range.startswith("bytes 1000-2000/"), f"Unexpected Content-Range: {content_range}"
    print(f"[playback] Seek Range 1000-2000 succeeded: {content_range}")

    print("\n=========================================")
    print("ALL DOCKER SERIES & PLAYBACK TESTS PASSED!")
    print("=========================================\n")

def main():
    try:
        clean_docker()
        setup_fixtures()
        run_container()
        wait_for_health()
        bootstrap_admin()
        run_tests()
    finally:
        clean_docker()
        if os.path.exists(TEST_DIR):
            shutil.rmtree(TEST_DIR, ignore_errors=True)

if __name__ == "__main__":
    main()
