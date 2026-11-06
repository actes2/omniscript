#!/usr/bin/env python3
"""
OmniScript Training Asset Distribution Server
=============================================
Lightweight Flask microservice to serve the training script and dataset
to your GPU desktop, and accept the trained .gguf model back via HTTP POST.
"""

import os
from pathlib import Path
from flask import Flask, send_file, request, jsonify

app = Flask(__name__)

BASE_DIR = Path(__file__).resolve().parent.parent
SCRIPTS_DIR = BASE_DIR / "scripts"
OUTPUTS_DIR = BASE_DIR / "tests" / "outputs"
MODELS_DIR = BASE_DIR / "models"
MODELS_DIR.mkdir(parents=True, exist_ok=True)

TRAIN_SCRIPT = SCRIPTS_DIR / "train_omni_model.py"
DATASET_FILE = OUTPUTS_DIR / "synthetic_training_dataset.jsonl"


@app.route("/")
def index():
    return """
    <!DOCTYPE html>
    <html>
    <head><title>OmniScript Training Assets</title></head>
    <body style="font-family: monospace; max-width: 800px; margin: 40px auto; line-height: 1.6;">
        <h2>OmniScript Training Distribution Server</h2>
        <p>Surface Host IP: <b>192.168.1.149:8080</b></p>
        
        <h3>Download Assets to PC:</h3>
        <ul>
            <li><a href="/download/train_omni_model.py">train_omni_model.py</a> (LoRA Training Script)</li>
            <li><a href="/download/synthetic_training_dataset.jsonl">synthetic_training_dataset.jsonl</a> (2,000 Verified Samples)</li>
        </ul>

        <h3>Quick Curl Commands on PC:</h3>
        <pre style="background: #eee; padding: 12px; border-radius: 4px;">
curl -O http://192.168.1.149:8080/download/train_omni_model.py
curl -O http://192.168.1.149:8080/download/synthetic_training_dataset.jsonl
        </pre>

        <h3>Upload Trained .gguf Back to Surface:</h3>
        <pre style="background: #eee; padding: 12px; border-radius: 4px;">
curl -F "file=@omni-model-q4_k_m.gguf" http://192.168.1.149:8080/upload
        </pre>
    </body>
    </html>
    """


@app.route("/download/train_omni_model.py", methods=["GET"])
def download_script():
    if not TRAIN_SCRIPT.exists():
        return jsonify({"error": "Training script not found"}), 404
    return send_file(TRAIN_SCRIPT, as_attachment=True, download_name="train_omni_model.py")


@app.route("/download/synthetic_training_dataset.jsonl", methods=["GET"])
def download_dataset():
    if not DATASET_FILE.exists():
        return jsonify({"error": "Dataset file not found"}), 404
    return send_file(DATASET_FILE, as_attachment=True, download_name="synthetic_training_dataset.jsonl")


@app.route("/upload", methods=["POST"])
def upload_model():
    if "file" not in request.files:
        return jsonify({"error": "No file field in request"}), 400

    file = request.files["file"]
    if file.filename == "":
        return jsonify({"error": "No filename provided"}), 400

    filename = Path(file.filename).name
    dest_path = MODELS_DIR / filename
    file.save(dest_path)

    size_mb = dest_path.stat().st_size / (1024 * 1024)
    print(f"[UPLOAD] Successfully saved {filename} ({size_mb:.2f} MB) to {dest_path}")

    return jsonify({
        "status": "success",
        "saved_to": str(dest_path),
        "size_mb": round(size_mb, 2),
    }), 200


if __name__ == "__main__":
    port = int(os.environ.get("PORT", 8080))
    print(f"[*] Starting OmniScript Flask server on http://0.0.0.0:{port}...")
    app.run(host="0.0.0.0", port=port, debug=False)
