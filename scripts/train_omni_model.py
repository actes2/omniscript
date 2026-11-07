#!/usr/bin/env python3
"""
OmniScript LLVM-IR Compiler LoRA Training Script
================================================
Designed to run on a dedicated GPU workstation (NVIDIA CUDA).
Trains an open LLM (Qwen2.5-Coder-1.5B or Meta LLM-Compiler 7B) on verified
<omni_ir_task> <-> LLVM IR basic block completions, then merges the adapter
and prepares it for GGUF quantization.

Dependencies on your PC:
    pip install torch transformers peft datasets trl accelerate bitsandbytes
"""

import argparse
import json
import os
import sys
from pathlib import Path


def parse_args():
    parser = argparse.ArgumentParser(description="Fine-tune an LLM on OmniScript LLVM-IR generation tasks.")
    parser.add_argument(
        "--dataset",
        type=str,
        default="synthetic_training_dataset.jsonl",
        help="Path to synthetic_training_dataset.jsonl",
    )
    parser.add_argument(
        "--base_model",
        type=str,
        default="Qwen/Qwen2.5-Coder-1.5B",
        help="Base model identifier (e.g., 'Qwen/Qwen2.5-Coder-1.5B' or 'facebook/llm-compiler-7b')",
    )
    parser.add_argument(
        "--output_dir",
        type=str,
        default="./omni_model_output",
        help="Directory to save adapter checkpoints and merged model",
    )
    parser.add_argument(
        "--epochs",
        type=int,
        default=3,
        help="Number of training epochs",
    )
    parser.add_argument(
        "--batch_size",
        type=int,
        default=4,
        help="Per-device batch size",
    )
    parser.add_argument(
        "--grad_accum",
        type=int,
        default=4,
        help="Gradient accumulation steps (effective batch size = batch_size * grad_accum)",
    )
    parser.add_argument(
        "--lr",
        type=float,
        default=2e-4,
        help="Learning rate",
    )
    parser.add_argument(
        "--max_seq_len",
        type=int,
        default=512,
        help="Max sequence length in tokens (OmniScript IR blocks are compact, ~200-350 tokens)",
    )
    parser.add_argument(
        "--lora_r",
        type=int,
        default=16,
        help="LoRA rank dimension",
    )
    parser.add_argument(
        "--lora_alpha",
        type=int,
        default=32,
        help="LoRA alpha scaling factor",
    )
    return parser.parse_args()


def load_dataset_records(dataset_path: str):
    if not os.path.exists(dataset_path):
        raise FileNotFoundError(f"Dataset file not found at: {dataset_path}")

    records = []
    with open(dataset_path, "r", encoding="utf-8") as f:
        for line_num, line in enumerate(f, 1):
            line = line.trim() if hasattr(line, "trim") else line.strip()
            if not line:
                continue
            try:
                item = json.loads(line)
                if item.get("verified", False):
                    records.append(item)
            except json.JSONDecodeError as e:
                print(f"[WARN] Skipping corrupted JSON at line {line_num}: {e}")

    print(f"[DATASET] Loaded {len(records)} verified training samples from {dataset_path}")
    return records


def main():
    args = parse_args()

    print("=" * 70)
    print(" OmniScript LLVM-IR Model Training Harness (GPU)")
    print(f" Base Model:      {args.base_model}")
    print(f" Dataset:         {args.dataset}")
    print(f" Output Dir:      {args.output_dir}")
    print(f" Epochs:          {args.epochs}")
    print(f" Batch Size:      {args.batch_size} (Grad Accum: {args.grad_accum})")
    print(f" LoRA Config:     r={args.lora_r}, alpha={args.lora_alpha}")
    print("=" * 70)

    try:
        import torch
        from datasets import Dataset
        from peft import LoraConfig, TaskType, get_peft_model
        from transformers import (
            AutoModelForCausalLM,
            AutoTokenizer,
            DataCollatorForSeq2Seq,
            Trainer,
            TrainingArguments,
        )
    except ImportError as e:
        print("\n[ERROR] Missing required Python machine learning libraries.")
        print("Please install them on your GPU PC via:")
        print("  pip install torch transformers peft datasets trl accelerate bitsandbytes\n")
        sys.exit(1)

    if not torch.cuda.is_available():
        print("[WARN] No NVIDIA CUDA device detected. Training on CPU will be extremely slow.")
    else:
        print(f"[DEVICE] Training on GPU: {torch.cuda.get_device_name(0)}")

    # 1. Load raw dataset records
    raw_records = load_dataset_records(args.dataset)
    if not raw_records:
        print("[ERROR] No verified records found to train on. Exiting.")
        sys.exit(1)

    # 2. Initialize Tokenizer & Model
    print(f"\n[MODEL] Loading base model and tokenizer: {args.base_model}...")
    tokenizer = AutoTokenizer.from_pretrained(args.base_model, trust_remote_code=True)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token

    compute_dtype = torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float16

    model = AutoModelForCausalLM.from_pretrained(
        args.base_model,
        torch_dtype=compute_dtype,
        device_map="auto",
        trust_remote_code=True,
    )

    # 3. Configure LoRA
    print("[PEFT] Initializing LoRA configuration...")
    peft_config = LoraConfig(
        task_type=TaskType.CAUSAL_LM,
        r=args.lora_r,
        lora_alpha=args.lora_alpha,
        lora_dropout=0.05,
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
        bias="none",
    )
    model = get_peft_model(model, peft_config)
    model.print_trainable_parameters()

    # 4. Format prompt/completion pairs
    system_prompt = (
        "You are the OmniScript LLVM IR Compiler Backend.\n"
        "Synthesize valid LLVM IR basic blocks matching the exact target ABI, register context, and intent.\n"
        "Output ONLY raw LLVM IR instructions without markdown formatting, code fences, or conversational prose."
    )

    formatted_samples = []
    for r in raw_records:
        user_prompt = r["prompt"].strip()
        assistant_resp = r["completion"].strip()

        # Build full text format
        if hasattr(tokenizer, "apply_chat_template") and tokenizer.chat_template:
            messages = [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
                {"role": "assistant", "content": assistant_resp},
            ]
            full_text = tokenizer.apply_chat_template(messages, tokenize=False)
        else:
            full_text = f"### System:\n{system_prompt}\n\n### User:\n{user_prompt}\n\n### Assistant:\n{assistant_resp}{tokenizer.eos_token}"

        formatted_samples.append({"text": full_text})

    dataset = Dataset.from_list(formatted_samples)

    def tokenize_fn(example):
        tokens = tokenizer(
            example["text"],
            truncation=True,
            max_length=args.max_seq_len,
            padding=False,
        )
        tokens["labels"] = tokens["input_ids"].copy()
        return tokens

    tokenized_dataset = dataset.map(tokenize_fn, remove_columns=["text"])

    # 5. Training Arguments
    training_args = TrainingArguments(
        output_dir=os.path.join(args.output_dir, "checkpoints"),
        num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size,
        gradient_accumulation_steps=args.grad_accum,
        learning_rate=args.lr,
        lr_scheduler_type="cosine",
        warmup_steps=20,
        logging_steps=10,
        save_strategy="epoch",
        fp16=(compute_dtype == torch.float16),
        bf16=(compute_dtype == torch.bfloat16),
        optim="adamw_torch",
        report_to="none",
    )

    data_collator = DataCollatorForSeq2Seq(
        tokenizer,
        pad_to_multiple_of=8,
        return_tensors="pt",
        padding=True,
    )

    trainer = Trainer(
        model=model,
        args=training_args,
        train_dataset=tokenized_dataset,
        data_collator=data_collator,
    )

    # 6. Execute Training
    print("\n[TRAIN] Beginning fine-tuning pass on GPU...")
    trainer.train()

    # 7. Save Adapter & Merge Model
    adapter_path = os.path.join(args.output_dir, "adapter")
    print(f"\n[SAVE] Saving LoRA adapter to: {adapter_path}")
    model.save_pretrained(adapter_path)
    tokenizer.save_pretrained(adapter_path)

    merged_path = os.path.join(args.output_dir, "merged")
    print(f"[MERGE] Merging LoRA adapter into base model weights -> {merged_path}...")
    merged_model = model.merge_and_unload()
    merged_model.save_pretrained(merged_path, safe_serialization=True)
    tokenizer.save_pretrained(merged_path)
    print("[SUCCESS] Merged standalone Hugging Face model saved successfully!")

    # 8. Print Quantization Guide for User
    print("\n" + "=" * 70)
    print(" GGUF Export & Quantization Instructions (for running on Surface)")
    print("=" * 70)
    print("To convert this merged model into GGUF for `omnic` / `llama.cpp` on your Surface:")
    print("")
    print("  1. Clone llama.cpp if not present:")
    print("     git clone https://github.com/ggerganov/llama.cpp && cd llama.cpp")
    print("")
    print("  2. Convert Hugging Face weights to fp16 GGUF:")
    print(f"     python3 convert_hf_to_gguf.py {merged_path} --outfile omni-model-f16.gguf")
    print("")
    print("  3. Quantize to high-efficiency Q4_K_M (or Q5_K_M):")
    print("     ./llama-quantize omni-model-f16.gguf omni-model-q4_k_m.gguf Q4_K_M")
    print("")
    print("  4. SCP/Copy omni-model-q4_k_m.gguf back to your Surface:")
    print("     scp omni-model-q4_k_m.gguf user@surface:/home/actes/projects/slop/omniscript/models/")
    print("=" * 70 + "\n")


if __name__ == "__main__":
    main()
