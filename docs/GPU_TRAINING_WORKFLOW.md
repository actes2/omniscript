# GPU Workstation Training & GGUF Quantization Workflow

This guide details the procedure for transferring OmniScript's Clang-verified synthetic dataset to a dedicated GPU workstation (NVIDIA CUDA), running LoRA fine-tuning, and exporting the quantized `.gguf` back to your Surface for in-process compilation.

---

## 1. Files to Transfer to Your GPU Workstation

From this repository on your Surface, copy two files to your GPU PC:
1. **Training Script**: [`scripts/train_omni_model.py`](file:///home/actes/projects/slop/omniscript/scripts/train_omni_model.py)
2. **Dataset File**: [`tests/outputs/synthetic_training_dataset.jsonl`](file:///home/actes/projects/slop/omniscript/tests/outputs/synthetic_training_dataset.jsonl)

Example transfer via `scp`:
```bash
scp scripts/train_omni_model.py tests/outputs/synthetic_training_dataset.jsonl user@gpu-desktop:~/omni_training/
```

---

## 2. On Your GPU Workstation: Environment Setup

Install the required Hugging Face ML stack (Python 3.10+ with CUDA):
```bash
pip install torch torchvision --index-url https://download.pytorch.org/whl/cu121
pip install transformers peft datasets trl accelerate bitsandbytes
```

---

## 3. Running Fine-Tuning

### Option A: Train on `Qwen/Qwen2.5-Coder-1.5B` (Recommended for Sub-Second Compiler JIT)
Takes ~10–15 minutes on an RTX 3080/4080/4090:
```bash
python3 train_omni_model.py \
    --base_model Qwen/Qwen2.5-Coder-1.5B \
    --dataset synthetic_training_dataset.jsonl \
    --output_dir ./omni_qwen_1.5b \
    --epochs 3 \
    --batch_size 4 \
    --grad_accum 4 \
    --lr 2e-4
```

### Option B: Train on `facebook/llm-compiler-7b`
```bash
python3 train_omni_model.py \
    --base_model facebook/llm-compiler-7b \
    --dataset synthetic_training_dataset.jsonl \
    --output_dir ./omni_llm_compiler_7b \
    --epochs 3 \
    --batch_size 2 \
    --grad_accum 8 \
    --lr 1e-4
```

The script will automatically:
1. Train the LoRA adapter on your GPU.
2. Save the adapter weights to `./omni_model_output/adapter`.
3. Merge the adapter back into the base model and write standard Hugging Face weights to `./omni_model_output/merged`.

---

## 4. Convert and Quantize to GGUF (on GPU PC)

On your GPU machine, build `llama.cpp` tools:
```bash
git clone https://github.com/ggerganov/llama.cpp
cd llama.cpp
cmake -B build
cmake --build build --config Release -j
```

### Step 1: Convert Merged HF Weights to GGUF (fp16)
```bash
python3 convert_hf_to_gguf.py ../omni_qwen_1.5b/merged --outfile omni-coder-1.5b-f16.gguf
```

### Step 2: Quantize to High-Efficiency `Q4_K_M`
```bash
./build/bin/llama-quantize omni-coder-1.5b-f16.gguf omni-coder-1.5b-q4_k_m.gguf Q4_K_M
```

---

## 5. Copy the GGUF Back to Your Surface

```bash
mkdir -p /home/actes/projects/slop/omniscript/models
scp omni-coder-1.5b-q4_k_m.gguf actes@surface:/home/actes/projects/slop/omniscript/models/
```

Once in place, `omnic` runs in-process LLVM IR synthesis directly from the quantized GGUF with sub-second latency and zero swapping.
