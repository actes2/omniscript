import os
import sys
import time
import socket
from flask import Flask, request, jsonify

app = Flask(__name__)
DEFAULT_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "models")
UPLOAD_DIR = os.environ.get("OMNI_MODEL_DIR", DEFAULT_DIR)
TARGET_FILE = os.path.join(UPLOAD_DIR, "omni-coder.gguf")

def get_local_ip():
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("8.8.8.8", 80))
        ip = s.getsockname()[0]
        s.close()
        return ip
    except Exception:
        return "127.0.0.1"

@app.route("/", methods=["GET"])
def index():
    local_ip = get_local_ip()
    return f"""
    <h2>OmniScript Model Upload Server</h2>
    <p>Target destination: <code>{TARGET_FILE}</code></p>
    <p>Upload via curl:</p>
    <pre>
curl -T model.gguf http://{local_ip}:8080/upload
# OR multipart:
curl -F "file=@model.gguf" http://{local_ip}:8080/upload
    </pre>
    """

@app.route("/upload", methods=["POST", "PUT"])
def upload():
    os.makedirs(UPLOAD_DIR, exist_ok=True)
    temp_file = TARGET_FILE + ".tmp"
    t0 = time.time()
    bytes_written = 0

    if "file" in request.files:
        f = request.files["file"]
        filename = f.filename or "omni-coder.gguf"
        print(f"[*] Incoming multipart upload: {filename}")
        with open(temp_file, "wb") as out:
            while True:
                chunk = f.read(1024 * 1024)
                if not chunk:
                    break
                out.write(chunk)
                bytes_written += len(chunk)
                print(f"\r  -> Written: {bytes_written / (1024*1024):.2f} MB", end="", flush=True)
    else:
        print("[*] Incoming binary stream upload")
        with open(temp_file, "wb") as out:
            while True:
                chunk = request.stream.read(1024 * 1024)
                if not chunk:
                    break
                out.write(chunk)
                bytes_written += len(chunk)
                print(f"\r  -> Written: {bytes_written / (1024*1024):.2f} MB", end="", flush=True)

    print()
    os.replace(temp_file, TARGET_FILE)
    elapsed = time.time() - t0
    mb = bytes_written / (1024 * 1024)
    speed = mb / max(elapsed, 0.001)
    print(f"[+] Upload complete: {mb:.2f} MB in {elapsed:.2f}s ({speed:.2f} MB/s) -> {TARGET_FILE}")

    return jsonify({
        "status": "success",
        "destination": TARGET_FILE,
        "size_mb": round(mb, 2),
        "elapsed_seconds": round(elapsed, 2),
        "speed_mb_s": round(speed, 2),
    })

if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8080"))
    local_ip = get_local_ip()
    print(f"[*] Starting OmniScript GGUF Upload Server on 0.0.0.0:{port}")
    print(f"[*] LAN endpoint: http://{local_ip}:{port}/upload")
    print(f"[*] Target destination: {TARGET_FILE}")
    app.run(host="0.0.0.0", port=port, threaded=True)

