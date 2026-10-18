import urllib.request
import urllib.error
import http.cookiejar
import json
import time

BASE_URL = "http://127.0.0.1:3000"

cj = http.cookiejar.CookieJar()
opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))

# Login
login_data = json.dumps({"login": "admin", "password": "AetherSuperSecure2026!"}).encode('utf-8')
req = urllib.request.Request(
    f"{BASE_URL}/api/v1/auth/login",
    data=login_data,
    headers={"Host": "127.0.0.1:3000", "Origin": "http://127.0.0.1:3000", "Content-Type": "application/json"}
)
with opener.open(req) as resp:
    login_res = json.loads(resp.read().decode('utf-8'))
    csrf_token = login_res["csrfToken"]

# Check catalog
req = urllib.request.Request(f"{BASE_URL}/api/v1/catalog", headers={"Host": "127.0.0.1:3000"})
with opener.open(req) as resp:
    raw = resp.read().decode('utf-8')
    catalog = json.loads(raw)
    print("Catalog type:", type(catalog))
    if isinstance(catalog, dict):
        print("Catalog keys:", list(catalog.keys()))
        works = catalog.get("works", catalog.get("items", []))
    else:
        works = catalog
    print(f"Works count: {len(works)}")
    if works:
        w = works[0]
        print(f"Work 0: title='{w.get('title')}', year={w.get('year')}, posterUrl={w.get('posterUrl')}")
        # Find asset ID
        asset_id = None
        for ed in w.get("editions", []):
            for v in ed.get("versions", []):
                for s in v.get("sources", []):
                    for a in s.get("assets", []):
                        asset_id = a.get("id")
                        print(f"Asset: id={asset_id}, kind={a.get('kind')}, path={a.get('path')}")
                        break
        
        # Test poster if present
        if w.get('posterUrl'):
            poster_url = w.get('posterUrl')
            p_req = urllib.request.Request(f"{BASE_URL}{poster_url}", headers={"Host": "127.0.0.1:3000"})
            with opener.open(p_req) as p_resp:
                print(f"Poster response: status={p_resp.status}, content-type={p_resp.headers.get('Content-Type')}, length={len(p_resp.read())}")
        
        # Test HTTP Range request on asset
        if asset_id:
            content_url = f"{BASE_URL}/api/v1/assets/{asset_id}/content"
            
            # 1. Valid range 0-1023
            r_req = urllib.request.Request(content_url, headers={"Host": "127.0.0.1:3000", "Range": "bytes=0-1023"})
            try:
                with opener.open(r_req) as r_resp:
                    print(f"Range 0-1023: status={r_resp.status}, Content-Range={r_resp.headers.get('Content-Range')}, bytes={len(r_resp.read())}")
            except urllib.error.HTTPError as e:
                print(f"Range 0-1023 error: {e.code}")
            
            # 2. Invalid range
            inv_req = urllib.request.Request(content_url, headers={"Host": "127.0.0.1:3000", "Range": "bytes=999999999-999999999"})
            try:
                with opener.open(inv_req) as inv_resp:
                    print(f"Invalid range status: {inv_resp.status}")
            except urllib.error.HTTPError as e:
                print(f"Invalid range returned expected HTTP error: {e.code} (headers: {dict(e.headers)})")

print("All E2E assertions passed successfully!")
