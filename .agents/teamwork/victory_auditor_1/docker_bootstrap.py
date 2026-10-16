import json
import time
import re

try:
    f = open(r'\\.\pipe\docker_engine', 'r+b', buffering=0)
    print("Successfully opened \\\\.\\pipe\\docker_engine")
    
    # 1. Create exec instance
    req_body = json.dumps({
        "AttachStdin": True,
        "AttachStdout": True,
        "AttachStderr": True,
        "Tty": True,
        "Cmd": ["media-bootstrap"]
    }).encode('utf-8')
    
    http_req = (
        b"POST /containers/aether/exec HTTP/1.1\r\n"
        b"Host: localhost\r\n"
        b"Content-Type: application/json\r\n"
        b"Content-Length: " + str(len(req_body)).encode('utf-8') + b"\r\n\r\n" + req_body
    )
    f.write(http_req)
    
    # Read response
    resp = b""
    while b"{" not in resp:
        chunk = f.read(512)
        if not chunk:
            break
        resp += chunk
    
    print("Create exec response:", resp.decode('utf-8', errors='ignore'))
    
    # Extract JSON object with regex
    m = re.search(rb'\{.*\}', resp)
    if not m:
        # maybe more data needed
        resp += f.read(512)
        m = re.search(rb'\{.*\}', resp)
    
    body = m.group(0).decode('utf-8')
    exec_id = json.loads(body)["Id"]
    print("Exec ID:", exec_id)
    
    # 2. Start exec instance with raw bidirectional stream
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
    
    # Read response headers until \r\n\r\n
    hdr = b""
    while b"\r\n\r\n" not in hdr:
        chunk = f_start.read(128)
        if not chunk:
            break
        hdr += chunk
    print("Header:", hdr.decode('utf-8', errors='ignore'))
    
    time.sleep(0.3)
    f_start.write(b"admin\n")
    time.sleep(0.3)
    f_start.write(b"AetherSuperSecure2026!\n")
    time.sleep(0.3)
    f_start.write(b"AetherSuperSecure2026!\n")
    time.sleep(0.8)
    
    output = f_start.read(2048)
    print("Result output:\n" + output.decode('utf-8', errors='ignore'))
    
except Exception as e:
    import traceback
    traceback.print_exc()
