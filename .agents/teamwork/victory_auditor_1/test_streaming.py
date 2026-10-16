import urllib.request
import urllib.error
import http.cookiejar
import json

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
    pass

# Check all works
req = urllib.request.Request(f"{BASE_URL}/api/v1/catalog", headers={"Host": "127.0.0.1:3000"})
with opener.open(req) as resp:
    catalog = json.loads(resp.read().decode('utf-8'))
    items = catalog["items"]
    print(f"Total items in catalog: {len(items)}")
    
    tested_range = False
    tested_poster = False
    
    for item in items:
        # Check poster
        if not tested_poster and item.get("posterUrl"):
            p_url = item["posterUrl"]
            p_req = urllib.request.Request(f"{BASE_URL}{p_url}", headers={"Host": "127.0.0.1:3000"})
            with opener.open(p_req) as p_resp:
                print(f"Verified poster for '{item.get('title')}': status={p_resp.status}, Content-Type={p_resp.headers.get('Content-Type')}, size={len(p_resp.read())} bytes")
                tested_poster = True
        
        # Check assets
        if not tested_range:
            for ed in item.get("editions", []):
                for v in ed.get("versions", []):
                    for s in v.get("sources", []):
                        for a in s.get("assets", []):
                            asset_id = a.get("id")
                            print(f"Testing streaming on asset: id={asset_id}, path={a.get('path')}")
                            content_url = f"{BASE_URL}/api/v1/assets/{asset_id}/content"
                            
                            # Range 0-1023
                            r_req = urllib.request.Request(content_url, headers={"Host": "127.0.0.1:3000", "Range": "bytes=0-1023"})
                            with opener.open(r_req) as r_resp:
                                assert r_resp.status == 206
                                cr = r_resp.headers.get("Content-Range")
                                data = r_resp.read()
                                print(f"HTTP Range 0-1023 verified: status=206, Content-Range='{cr}', read {len(data)} bytes")
                            
                            # Range 1024-4095
                            r_req2 = urllib.request.Request(content_url, headers={"Host": "127.0.0.1:3000", "Range": "bytes=1024-4095"})
                            with opener.open(r_req2) as r_resp2:
                                assert r_resp2.status == 206
                                cr2 = r_resp2.headers.get("Content-Range")
                                data2 = r_resp2.read()
                                print(f"HTTP Range 1024-4095 verified: status=206, Content-Range='{cr2}', read {len(data2)} bytes")
                            
                            # Invalid Range
                            inv_req = urllib.request.Request(content_url, headers={"Host": "127.0.0.1:3000", "Range": "bytes=999999999-999999999"})
                            try:
                                with opener.open(inv_req) as inv_resp:
                                    print("Unexpected success on invalid range:", inv_resp.status)
                            except urllib.error.HTTPError as e:
                                print(f"Invalid range rejection verified: HTTP {e.code} (Content-Range='{e.headers.get('Content-Range')}')")
                                assert e.code == 416
                            
                            tested_range = True
                            break
                        if tested_range: break
                    if tested_range: break
                if tested_range: break

print("Completed Asset & Poster checks successfully!")
