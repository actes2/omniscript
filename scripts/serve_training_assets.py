#!/usr/bin/env python3
"""
OmniScript Training Asset Distribution & GGUF Upload Server
===========================================================
Flask server exposing:
  - GET  /download/train_omni_model.py            (or /train_omni_model.py)
  - GET  /download/synthetic_training_dataset.jsonl (or /synthetic_training_dataset.jsonl)
  - GET  /download/imatrix_calib.txt              (or /imatrix_calib.txt)
  - GET  /bundle.tar.gz                           (one-shot tarball of all 3 assets)
  - PUT  /upload[/<filename>]                     (`curl -T omni-coder.gguf ...`)
  - POST /upload                                  (`curl -F "file=@omni-coder.gguf" ...`)
"""

import io
import os
import shutil
import socket
import tarfile
import time
from pathlib import Path
from flask import Flask, jsonify, request, send_file

app = Flask(__name__)

BASE_DIR = Path(__file__).resolve().parent.parent
SCRIPTS_DIR = BASE_DIR / "scripts"
OUTPUTS_DIR = BASE_DIR / "tests" / "outputs"
MODELS_DIR = BASE_DIR / "models"
MODELS_DIR.mkdir(parents=True, exist_ok=True)

TRAIN_SCRIPT = SCRIPTS_DIR / "train_omni_model.py"
DATASET_FILE = OUTPUTS_DIR / "synthetic_training_dataset.jsonl"
IMATRIX_FILE = OUTPUTS_DIR / "imatrix_calib.txt"
DEFAULT_DEST_MODEL = MODELS_DIR / "omni-coder.gguf"


def get_local_ip() -> str:
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.connect(("8.8.8.8", 80))
        return s.getsockname()[0]
    except Exception:
        return "127.0.0.1"
    finally:
        s.close()


def fmt_size(path: Path) -> str:
    if not path.exists():
        return "MISSING"
    sz = path.stat().st_size
    if sz >= 1024 * 1024:
        return f"{sz / (1024 * 1024):.2f} MB"
    return f"{sz / 1024:.1f} KB"


@app.route("/")
def index():
    host_ip = get_local_ip()
    port = int(os.environ.get("PORT", 8080))
    base = f"http://{host_ip}:{port}"
    return f"""<!DOCTYPE html>
<html>
<head><title>OmniScript Retraining Distribution Server</title></head>
<body style="font-family: monospace; max-width: 860px; margin: 36px auto; line-height: 1.55;">
    <h2>OmniScript 64-Bit Retraining Asset Server</h2>
    <p>Endpoint: <b>{base}</b></p>
    <ul>
        <li><a href="/download/train_omni_model.py">train_omni_model.py</a> ({fmt_size(TRAIN_SCRIPT)})</li>
        <li><a href="/download/synthetic_training_dataset.jsonl">synthetic_training_dataset.jsonl</a> ({fmt_size(DATASET_FILE)} — 6,000 Clang-verified samples)</li>
        <li><a href="/download/imatrix_calib.txt">imatrix_calib.txt</a> ({fmt_size(IMATRIX_FILE)} — 300 ChatML calibration traces)</li>
        <li><a href="/bundle.tar.gz">bundle.tar.gz</a> (All 3 files in a single archive)</li>
    </ul>

    <h3>1. Pull Assets on Lab-Box:</h3>
    <pre style="background: #f3f3f3; padding: 12px; border-radius: 4px;">curl -O {base}/download/train_omni_model.py
curl -O {base}/download/synthetic_training_dataset.jsonl
curl -O {base}/download/imatrix_calib.txt</pre>

    <h3>2. Push Quantized GGUF Back to Surface (`models/omni-coder.gguf`):</h3>
    <pre style="background: #f3f3f3; padding: 12px; border-radius: 4px;">curl -T omni-coder.gguf {base}/upload</pre>
</body>
</html>
"""


@app.route("/train_omni_model.py", methods=["GET"])
@app.route("/download/train_omni_model.py", methods=["GET"])
def download_script():
    if not TRAIN_SCRIPT.exists():
        return jsonify({"error": "train_omni_model.py not found"}), 404
    return send_file(TRAIN_SCRIPT, as_attachment=True, download_name="train_omni_model.py")


@app.route("/synthetic_training_dataset.jsonl", methods=["GET"])
@app.route("/download/synthetic_training_dataset.jsonl", methods=["GET"])
def download_dataset():
    if not DATASET_FILE.exists():
        return jsonify({"error": "synthetic_training_dataset.jsonl not found"}), 404
    return send_file(DATASET_FILE, as_attachment=True, download_name="synthetic_training_dataset.jsonl")


@app.route("/imatrix_calib.txt", methods=["GET"])
@app.route("/download/imatrix_calib.txt", methods=["GET"])
def download_imatrix():
    if not IMATRIX_FILE.exists():
        return jsonify({"error": "imatrix_calib.txt not found"}), 404
    return send_file(IMATRIX_FILE, as_attachment=True, download_name="imatrix_calib.txt")


@app.route("/bundle.tar.gz", methods=["GET"])
@app.route("/download/bundle.tar.gz", methods=["GET"])
def download_bundle():
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for p in (TRAIN_SCRIPT, DATASET_FILE, IMATRIX_FILE):
            if p.exists():
                tar.add(str(p), arcname=p.name)
    buf.seek(0)
    return send_file(buf, mimetype="application/gzip", as_attachment=True, download_name="omni_retrain_bundle.tar.gz")


@app.route("/upload", methods=["PUT", "POST"])
@app.route("/upload/<filename>", methods=["PUT", "POST"])
def upload_model(filename=None):
    t0 = time.time()
    target_name = Path(filename).name if filename else "omni-coder.gguf"
    if not target_name.endswith(".gguf"):
        target_name = "omni-coder.gguf"

    dest_path = MODELS_DIR / target_name
    temp_path = MODELS_DIR / f"{target_name}.tmp"
    backup_path = MODELS_DIR / f"{target_name}.bak"

    total_bytes = 0
    chunk_size = 4 * 1024 * 1024

    if request.method == "POST" and "file" in request.files:
        f = request.files["file"]
        if f.filename and f.filename.endswith(".gguf") and filename is None:
            # Default canonical target remains omni-coder.gguf unless a custom filename was requested in URL
            pass
        f.save(temp_path)
        total_bytes = temp_path.stat().st_size
    else:
        with open(temp_path, "wb") as out:
            while True:
                chunk = request.stream.read(chunk_size)
                if not chunk:
                    break
                out.write(chunk)
                total_bytes += len(chunk)

    if total_bytes == 0:
        if temp_path.exists():
            temp_path.unlink()
        return jsonify({"error": "Empty upload payload"}), 400

    if dest_path.exists() and not backup_path.exists():
        shutil.copy2(dest_path, backup_path)
        print(f"[BACKUP] Preserved previous model at {backup_path}")

    os.replace(temp_path, dest_path)
    elapsed = max(0.001, time.time() - t0)
    mb = total_bytes / (1024 * 1024)
    print(f"[UPLOAD COMPLETE] {dest_path} ({mb:.2f} MB in {elapsed:.1f}s, {mb / elapsed:.1f} MB/s)")

    return jsonify(
        {
            "status": "ok",
            "saved_to": str(dest_path),
            "size_mb": round(mb, 2),
            "elapsed_sec": round(elapsed, 2),
        }
    ), 200


if __name__ == "__main__":
    port = int(os.environ.get("PORT", 8080))
    ip = get_local_ip()
    print("=" * 68, flush=True)
    print(" OmniScript Retraining Asset & GGUF Upload Server Ready!", flush=True)
    print(f" LAN Endpoint: http://{ip}:{port}", flush=True)
    print("=" * 68, flush=True)
    app.run(host="0.0.0.0", port=port, threaded=True)
