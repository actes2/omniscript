#!/usr/bin/env bash
# ============================================================================
# OmniScript Standalone GPU Server Setup, Train, Quantize & Upload Script
# Assumes ONLY python3 + curl + git (+ gcc/g++ for llama.cpp) on a fresh box.
#
# Usage (one-liner from the GPU server):
#   curl -sSL http://<HOST_IP>:8080/setup_and_train_gpu.sh | bash -s -- http://<HOST_IP>:8080
# ============================================================================
set -euo pipefail

SERVER_URL="${1:-}"
if [ -z "$SERVER_URL" ]; then
    echo "Usage: $0 http://<asset-server-ip>:8080 [base_model] [epochs] [orpo_epochs]"
    exit 1
fi
BASE_MODEL="${2:-Qwen/Qwen2.5-Coder-1.5B}"
EPOCHS="${3:-3}"
ORPO_EPOCHS="${4:-1}"
WORK_DIR="${OMNI_TRAIN_DIR:-$PWD/omni_retrain_work}"

echo "========================================================================"
echo " OmniScript Zero-Dep GPU Retraining Pipeline"
echo " Asset Server : $SERVER_URL"
echo " Base Model   : $BASE_MODEL (SFT=$EPOCHS ep, ORPO=$ORPO_EPOCHS ep)"
echo " Work Dir     : $WORK_DIR"
echo "========================================================================"

mkdir -p "$WORK_DIR"
cd "$WORK_DIR"

echo "[1/7] Pulling training bundle from $SERVER_URL/bundle.tar.gz ..."
curl -fSL "$SERVER_URL/bundle.tar.gz" -o omni_retrain_bundle.tar.gz
tar -xzf omni_retrain_bundle.tar.gz
ls -lh train_omni_model.py synthetic_training_dataset.jsonl imatrix_calib.txt

echo "[2/7] Creating isolated Python virtual environment (./venv) ..."
if [ ! -d venv ]; then
    python3 -m venv venv
fi
# shellcheck disable=SC1091
source venv/bin/activate
pip install --upgrade pip wheel setuptools

echo "[3/7] Installing PyTorch (auto-detecting CUDA) + HF/TRL/GGUF stack ..."
if command -v nvidia-smi >/dev/null 2>&1; then
    echo "  -> NVIDIA GPU detected:"
    nvidia-smi --query-gpu=name,memory.total --format=csv,noheader || true
    pip install --upgrade torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu124
else
    echo "  -> No nvidia-smi found; installing standard PyTorch"
    pip install --upgrade torch
fi
pip install --upgrade \
    transformers \
    datasets \
    accelerate \
    peft \
    trl \
    sentencepiece \
    protobuf \
    gguf \
    numpy \
    cmake \
    ninja

echo "[4/7] Cloning & building llama.cpp (llama-quantize, llama-imatrix) ..."
if [ ! -d llama.cpp ]; then
    git clone --depth 1 https://github.com/ggml-org/llama.cpp.git
fi
CMAKE_BIN="$(command -v cmake || echo "$WORK_DIR/venv/bin/cmake")"
CUDA_FLAG="-DGGML_CUDA=OFF"
if command -v nvcc >/dev/null 2>&1; then
    CUDA_FLAG="-DGGML_CUDA=ON"
fi
"$CMAKE_BIN" -S llama.cpp -B llama.cpp/build \
    -DCMAKE_BUILD_TYPE=Release \
    "$CUDA_FLAG" \
    -DLLAMA_CURL=OFF \
    -DBUILD_SHARED_LIBS=OFF
"$CMAKE_BIN" --build llama.cpp/build --target llama-quantize llama-imatrix -j"$(nproc)"

echo "[5/7] Running Stage-1 SFT + Stage-2 ORPO Training ..."
python3 train_omni_model.py \
    --base_model "$BASE_MODEL" \
    --dataset synthetic_training_dataset.jsonl \
    --output_dir ./omni_qwen_1.5b \
    --epochs "$EPOCHS" \
    --orpo_epochs "$ORPO_EPOCHS"

MERGED_DIR="./omni_qwen_1.5b/merged"
if [ ! -d "$MERGED_DIR" ]; then
    MERGED_DIR="./omni_qwen_1.5b"
fi

echo "[6/7] Converting to FP16 GGUF -> imatrix calibration -> Q4_K_M quantization ..."
python3 llama.cpp/convert_hf_to_gguf.py "$MERGED_DIR" \
    --outfile omni-coder-f16.gguf \
    --outtype f16

./llama.cpp/build/bin/llama-imatrix \
    -m omni-coder-f16.gguf \
    -f imatrix_calib.txt \
    -o imatrix.dat \
    --chunks 100

./llama.cpp/build/bin/llama-quantize \
    --imatrix imatrix.dat \
    omni-coder-f16.gguf \
    omni-coder.gguf \
    Q4_K_M

ls -lh omni-coder.gguf

echo "[7/7] Uploading quantized omni-coder.gguf back to $SERVER_URL/upload ..."
curl -fSL -T omni-coder.gguf "$SERVER_URL/upload"
echo ""
echo "========================================================================"
echo " DONE! omni-coder.gguf trained, quantized (Q4_K_M), and uploaded."
echo "========================================================================"
