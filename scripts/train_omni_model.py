#!/usr/bin/env python3
"""
OmniScript LLVM-IR Compiler Retraining Harness (Qwen2.5-Coder-1.5B)
===================================================================
Two-stage fine-tuning pipeline tailored for OmniScript's 64-bit / IEEE-754 /
setjmp / Polyglot FFI backbone:
  - Stage 1: Completion-only Masked SFT (exact ChatML byte-parity with
             `CandleGgufEngine`, `labels=-100` on prompt tokens, per-record
             system prompt supporting both `<omni_ir_task>` lowering and
             `infer_dialect` classification).
  - Stage 2: Reference-free Monolithic ORPO preference pass over contrastive
             `(completion, rejected_completion)` pairs to suppress 32-bit `i32`
             pointer truncation, C-style casts, and markdown code fences.
  - Exports `imatrix_calib.txt` directly into `--output_dir` for `llama-imatrix`
    importance-weighted GGUF quantization (`Q5_K_M` / `Q4_K_M`).

Dependencies on GPU lab-box:
    pip install torch transformers peft datasets accelerate bitsandbytes
"""

import argparse
import gc
import json
import os
import sys
from pathlib import Path

DEFAULT_IR_SYSTEM_PROMPT = (
    "You are the OmniScript LLVM IR Compiler Backend.\n"
    "Synthesize valid LLVM IR basic blocks matching the exact target ABI, register context, and intent.\n"
    "Output ONLY raw LLVM IR instructions without markdown formatting, code fences, or conversational prose."
)


def parse_args():
    parser = argparse.ArgumentParser(
        description="Fine-tune Qwen2.5-Coder-1.5B on verified 64-bit OmniScript LLVM-IR & dialect tasks."
    )
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
        help="HuggingFace model ID or local path (default: Qwen/Qwen2.5-Coder-1.5B)",
    )
    parser.add_argument(
        "--output_dir",
        type=str,
        default="./omni_model_output",
        help="Directory to save adapter checkpoints, merged fp16/bf16 weights, and imatrix_calib.txt",
    )
    parser.add_argument(
        "--epochs",
        type=int,
        default=3,
        help="Stage-1 completion-masked SFT epochs (default: 3)",
    )
    parser.add_argument(
        "--orpo_epochs",
        type=int,
        default=1,
        help="Stage-2 ORPO contrastive preference epochs on rejected_completion pairs (default: 1)",
    )
    parser.add_argument(
        "--no_orpo",
        action="store_true",
        help="Skip Stage-2 ORPO contrastive pass and run Stage-1 masked SFT only",
    )
    parser.add_argument(
        "--orpo_beta",
        type=float,
        default=0.12,
        help="ORPO log-odds penalty weight beta (default: 0.12)",
    )
    parser.add_argument(
        "--batch_size",
        type=int,
        default=4,
        help="Per-device micro-batch size (default: 4)",
    )
    parser.add_argument(
        "--grad_accum",
        type=int,
        default=4,
        help="Gradient accumulation steps (effective batch = batch_size * grad_accum)",
    )
    parser.add_argument(
        "--lr",
        type=float,
        default=2e-4,
        help="Stage-1 SFT peak learning rate (default: 2e-4)",
    )
    parser.add_argument(
        "--max_seq_len",
        type=int,
        default=1024,
        help="Max sequence length in tokens (default: 1024 for drilled_context + compiler_diagnostic)",
    )
    parser.add_argument(
        "--lora_r",
        type=int,
        default=32,
        help="LoRA rank dimension (default: 32)",
    )
    parser.add_argument(
        "--lora_alpha",
        type=int,
        default=64,
        help="LoRA alpha scaling factor (default: 64)",
    )
    parser.add_argument(
        "--load_in_4bit",
        "--4bit",
        action="store_true",
        help="Enable QLoRA 4-bit NF4 base weights for constrained VRAM (<12 GB)",
    )
    parser.add_argument(
        "--upload_url",
        type=str,
        default="http://192.168.1.149:8080/upload",
        help="Surface upload endpoint printed in post-training instructions",
    )
    return parser.parse_args()


def load_dataset_records(dataset_path: str):
    if not os.path.exists(dataset_path):
        raise FileNotFoundError(f"Dataset file not found at: {dataset_path}")

    records = []
    with open(dataset_path, "r", encoding="utf-8") as f:
        for line_num, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                item = json.loads(line)
                if item.get("verified", False):
                    records.append(item)
            except json.JSONDecodeError as e:
                print(f"[WARN] Skipping malformed JSON at line {line_num}: {e}")

    repairs = sum(1 for r in records if "<compiler_diagnostic>" in r.get("prompt", ""))
    orpo_cnt = sum(1 for r in records if r.get("rejected_completion"))
    dialects = sum(1 for r in records if r.get("domain") == "polyglot_dialect_classifier")
    print(
        f"[DATASET] Loaded {len(records)} verified samples from {dataset_path} "
        f"(self-repair: {repairs}, ORPO contrastive: {orpo_cnt}, dialect-classifier: {dialects})"
    )
    return records


def export_imatrix_calibration(records, out_path: Path, max_samples: int = 300):
    out_path.parent.mkdir(parents=True, exist_ok=True)
    step = max(1, len(records) // max(1, max_samples))
    written = 0
    with open(out_path, "w", encoding="utf-8") as f:
        for r in records[::step][:max_samples]:
            sys_prompt = r.get("system_prompt") or DEFAULT_IR_SYSTEM_PROMPT
            prompt = r["prompt"].strip()
            comp = r["completion"].strip()
            f.write(
                f"<|im_start|>system\n{sys_prompt}<|im_end|>\n"
                f"<|im_start|>user\n{prompt}<|im_end|>\n"
                f"<|im_start|>assistant\n{comp}\n<|im_end|>\n\n"
            )
            written += 1
    print(f"[IMATRIX] Exported {written} ChatML calibration traces -> {out_path}")


def encode_chatml_pair(tokenizer, sys_prompt: str, user_prompt: str, assistant_resp: str, max_seq_len: int):
    """
    Tokenizes into exact CandleGgufEngine ChatML layout and masks all system/user
    prompt tokens with `-100` so 100% of gradient updates target LLVM IR / dialect output.
    """
    prompt_prefix = (
        f"<|im_start|>system\n{sys_prompt}<|im_end|>\n"
        f"<|im_start|>user\n{user_prompt}<|im_end|>\n"
        f"<|im_start|>assistant\n"
    )
    resp_suffix = f"{assistant_resp}\n<|im_end|>"

    prompt_ids = tokenizer.encode(prompt_prefix, add_special_tokens=False)
    resp_ids = tokenizer.encode(resp_suffix, add_special_tokens=False)

    if len(resp_ids) >= max_seq_len - 32:
        resp_ids = resp_ids[: max_seq_len - 32]
    max_prompt_budget = max_seq_len - len(resp_ids)
    if len(prompt_ids) > max_prompt_budget:
        # Preserve the tail of the prompt (<live_registers>, <intent>, <compiler_diagnostic>, assistant header)
        prompt_ids = prompt_ids[-max_prompt_budget:]

    input_ids = prompt_ids + resp_ids
    attention_mask = [1] * len(input_ids)
    labels = ([-100] * len(prompt_ids)) + resp_ids
    return {
        "input_ids": input_ids,
        "attention_mask": attention_mask,
        "labels": labels,
    }


def main():
    args = parse_args()
    out_dir = Path(args.output_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    print("=" * 74)
    print(" OmniScript 64-Bit LLVM-IR Retraining Pipeline (Qwen2.5-Coder-1.5B)")
    print(f" Base Model:      {args.base_model}")
    print(f" Dataset:         {args.dataset}")
    print(f" Output Dir:      {args.output_dir}")
    print(f" Stage-1 SFT:     {args.epochs} epochs @ lr={args.lr} (completion-only loss mask)")
    print(f" Stage-2 ORPO:    {'DISABLED' if args.no_orpo else f'{args.orpo_epochs} epoch(s), beta={args.orpo_beta}'}")
    print(f" Batch / Accum:   {args.batch_size} x {args.grad_accum} = {args.batch_size * args.grad_accum}")
    print(f" Max Seq Len:     {args.max_seq_len} | LoRA r={args.lora_r}, alpha={args.lora_alpha}")
    print("=" * 74)

    try:
        import torch
        import torch.nn.functional as F
        from datasets import Dataset
        from peft import LoraConfig, TaskType, get_peft_model
        from transformers import (
            AutoModelForCausalLM,
            AutoTokenizer,
            DataCollatorForSeq2Seq,
            Trainer,
            TrainingArguments,
        )
    except ImportError as exc:
        print(f"\n[ERROR] Missing required ML package: {exc}")
        print("Install on your lab-box via:")
        print("  pip install torch transformers peft datasets accelerate bitsandbytes\n")
        sys.exit(1)

    if torch.cuda.is_available():
        print(f"[DEVICE] CUDA GPU: {torch.cuda.get_device_name(0)}")
    else:
        print("[WARN] CUDA not detected; CPU training will be very slow.")

    # 1. Load dataset & immediately write imatrix calibration corpus
    raw_records = load_dataset_records(args.dataset)
    if not raw_records:
        print("[ERROR] Dataset contains 0 verified samples. Aborting.")
        sys.exit(1)

    imatrix_calib_path = out_dir / "imatrix_calib.txt"
    export_imatrix_calibration(raw_records, imatrix_calib_path, max_samples=300)

    # 2. Load Tokenizer & verify ChatML special token IDs
    print(f"\n[MODEL] Loading tokenizer & base weights: {args.base_model}...")
    tokenizer = AutoTokenizer.from_pretrained(args.base_model, trust_remote_code=True)
    if tokenizer.pad_token is None:
        tokenizer.pad_token = tokenizer.eos_token
    tokenizer.padding_side = "right"

    im_start_ids = tokenizer.encode("<|im_start|>", add_special_tokens=False)
    im_end_ids = tokenizer.encode("<|im_end|>", add_special_tokens=False)
    print(f"[TOKENIZER] ChatML token parity check: <|im_start|>={im_start_ids}, <|im_end|>={im_end_ids}")

    use_bf16 = torch.cuda.is_available() and torch.cuda.is_bf16_supported()
    compute_dtype = torch.bfloat16 if use_bf16 else torch.float16

    quant_config = None
    if args.load_in_4bit:
        from peft import prepare_model_for_kbit_training
        from transformers import BitsAndBytesConfig

        print("[QLoRA] Enabling 4-bit NormalFloat (NF4) double quantization...")
        quant_config = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=compute_dtype,
            bnb_4bit_use_double_quant=True,
        )

    model = AutoModelForCausalLM.from_pretrained(
        args.base_model,
        quantization_config=quant_config,
        torch_dtype=compute_dtype if not args.load_in_4bit else None,
        device_map="auto",
        trust_remote_code=True,
    )
    model.config.use_cache = False

    if args.load_in_4bit:
        model = prepare_model_for_kbit_training(model)
    else:
        model.gradient_checkpointing_enable()
        model.enable_input_require_grads()

    # 3. Attach LoRA across all attention + SwiGLU MLP projections
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

    # 4. Build Stage-1 Completion-Masked SFT Dataset
    sft_rows = []
    for r in raw_records:
        sys_prompt = r.get("system_prompt") or DEFAULT_IR_SYSTEM_PROMPT
        encoded = encode_chatml_pair(
            tokenizer,
            sys_prompt,
            r["prompt"].strip(),
            r["completion"].strip(),
            args.max_seq_len,
        )
        sft_rows.append(encoded)

    sft_dataset = Dataset.from_list(sft_rows).shuffle(seed=42)
    eff_batch = max(1, args.batch_size * args.grad_accum)
    sft_total_steps = max(1, (len(sft_dataset) // eff_batch) * args.epochs)

    sft_args = TrainingArguments(
        output_dir=str(out_dir / "checkpoints_sft"),
        num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size,
        gradient_accumulation_steps=args.grad_accum,
        learning_rate=args.lr,
        lr_scheduler_type="cosine",
        warmup_steps=max(10, int(0.04 * sft_total_steps)),
        logging_steps=20,
        save_strategy="epoch",
        fp16=(compute_dtype == torch.float16 and torch.cuda.is_available()),
        bf16=(compute_dtype == torch.bfloat16 and torch.cuda.is_available()),
        optim="adamw_torch",
        report_to="none",
        remove_unused_columns=False,
    )

    sft_collator = DataCollatorForSeq2Seq(
        tokenizer,
        pad_to_multiple_of=8,
        return_tensors="pt",
        padding=True,
        label_pad_token_id=-100,
    )

    sft_trainer = Trainer(
        model=model,
        args=sft_args,
        train_dataset=sft_dataset,
        data_collator=sft_collator,
    )

    print("\n[STAGE 1/2] Running Completion-Masked SFT...")
    sft_trainer.train()

    # 5. Stage-2 Reference-Free Monolithic ORPO Contrastive Pass
    contrastive_records = [r for r in raw_records if r.get("rejected_completion")]
    if not args.no_orpo and args.orpo_epochs > 0 and contrastive_records:
        print(
            f"\n[STAGE 2/2] Running Reference-Free ORPO Preference Pass on "
            f"{len(contrastive_records)} contrastive pairs (beta={args.orpo_beta})..."
        )
        try:
            orpo_rows = []
            for r in contrastive_records:
                sys_prompt = r.get("system_prompt") or DEFAULT_IR_SYSTEM_PROMPT
                chosen = encode_chatml_pair(
                    tokenizer,
                    sys_prompt,
                    r["prompt"].strip(),
                    r["completion"].strip(),
                    args.max_seq_len,
                )
                rejected = encode_chatml_pair(
                    tokenizer,
                    sys_prompt,
                    r["prompt"].strip(),
                    r["rejected_completion"].strip(),
                    args.max_seq_len,
                )
                orpo_rows.append(
                    {
                        "chosen_input_ids": chosen["input_ids"],
                        "chosen_attention_mask": chosen["attention_mask"],
                        "chosen_labels": chosen["labels"],
                        "rejected_input_ids": rejected["input_ids"],
                        "rejected_attention_mask": rejected["attention_mask"],
                        "rejected_labels": rejected["labels"],
                    }
                )

            orpo_dataset = Dataset.from_list(orpo_rows).shuffle(seed=1337)
            pad_id = tokenizer.pad_token_id

            def orpo_collator(features):
                def pad_seqs(seqs, fill_val):
                    max_len = max(len(s) for s in seqs)
                    max_len = ((max_len + 7) // 8) * 8
                    return torch.tensor([s + [fill_val] * (max_len - len(s)) for s in seqs], dtype=torch.long)

                return {
                    "chosen_input_ids": pad_seqs([f["chosen_input_ids"] for f in features], pad_id),
                    "chosen_attention_mask": pad_seqs([f["chosen_attention_mask"] for f in features], 0),
                    "chosen_labels": pad_seqs([f["chosen_labels"] for f in features], -100),
                    "rejected_input_ids": pad_seqs([f["rejected_input_ids"] for f in features], pad_id),
                    "rejected_attention_mask": pad_seqs([f["rejected_attention_mask"] for f in features], 0),
                    "rejected_labels": pad_seqs([f["rejected_labels"] for f in features], -100),
                }

            def sequence_logps_and_nll(logits, labels):
                shift_logits = logits[:, :-1, :].contiguous()
                shift_labels = labels[:, 1:].contiguous()
                mask = shift_labels != -100
                bsz, seqlen, vocab = shift_logits.shape
                token_nll = F.cross_entropy(
                    shift_logits.view(-1, vocab),
                    shift_labels.view(-1),
                    ignore_index=-100,
                    reduction="none",
                ).view(bsz, seqlen)
                token_logps = -token_nll.float()
                avg_logps = token_logps.sum(dim=-1) / mask.sum(dim=-1).clamp(min=1)
                nll = token_nll.float().sum() / mask.sum().clamp(min=1)
                return avg_logps, nll

            class OmniOrpoTrainer(Trainer):
                def compute_loss(self, model_ref, inputs, return_outputs=False, **kwargs):
                    out_w = model_ref(
                        input_ids=inputs["chosen_input_ids"],
                        attention_mask=inputs["chosen_attention_mask"],
                    )
                    logp_w, sft_loss = sequence_logps_and_nll(out_w.logits, inputs["chosen_labels"])
                    if not return_outputs:
                        del out_w

                    out_l = model_ref(
                        input_ids=inputs["rejected_input_ids"],
                        attention_mask=inputs["rejected_attention_mask"],
                    )
                    logp_l, _ = sequence_logps_and_nll(out_l.logits, inputs["rejected_labels"])
                    del out_l

                    logp_w_c = logp_w.clamp(max=-1e-6)
                    logp_l_c = logp_l.clamp(max=-1e-6)
                    log_odds_w = logp_w_c - torch.log1p(-torch.exp(logp_w_c))
                    log_odds_l = logp_l_c - torch.log1p(-torch.exp(logp_l_c))
                    or_loss = -F.logsigmoid(log_odds_w - log_odds_l).mean()

                    loss = sft_loss + (args.orpo_beta * or_loss)
                    return (loss, out_w) if return_outputs else loss

            orpo_batch = 1
            orpo_accum = max(1, args.batch_size * args.grad_accum)
            orpo_steps = max(1, (len(orpo_dataset) // orpo_accum) * args.orpo_epochs)
            orpo_args = TrainingArguments(
                output_dir=str(out_dir / "checkpoints_orpo"),
                num_train_epochs=args.orpo_epochs,
                per_device_train_batch_size=orpo_batch,
                gradient_accumulation_steps=orpo_accum,
                learning_rate=4e-5,
                lr_scheduler_type="cosine",
                warmup_steps=max(5, int(0.05 * orpo_steps)),
                logging_steps=20,
                save_strategy="no",
                fp16=(compute_dtype == torch.float16 and torch.cuda.is_available()),
                bf16=(compute_dtype == torch.bfloat16 and torch.cuda.is_available()),
                optim="adamw_torch",
                report_to="none",
                remove_unused_columns=False,
            )

            orpo_trainer = OmniOrpoTrainer(
                model=model,
                args=orpo_args,
                train_dataset=orpo_dataset,
                data_collator=orpo_collator,
            )
            orpo_trainer.train()
        except Exception as exc:
            print(f"[WARN] Stage-2 ORPO encountered an issue ({exc}); proceeding with Stage-1 SFT adapter.")

    # 6. Save Adapter & Merge Standalone fp16/bf16 HuggingFace Checkpoint
    adapter_path = out_dir / "adapter"
    print(f"\n[SAVE] Saving trained LoRA adapter -> {adapter_path}")
    model.save_pretrained(str(adapter_path))
    tokenizer.save_pretrained(str(adapter_path))

    merged_path = out_dir / "merged"
    print(f"[MERGE] Merging LoRA delta into unquantized base weights -> {merged_path}...")
    if args.load_in_4bit:
        from peft import PeftModel

        del model
        gc.collect()
        if torch.cuda.is_available():
            torch.cuda.empty_cache()

        base_reload = AutoModelForCausalLM.from_pretrained(
            args.base_model,
            torch_dtype=torch.float16,
            device_map="cpu",
            trust_remote_code=True,
        )
        peft_reload = PeftModel.from_pretrained(base_reload, str(adapter_path))
        merged_model = peft_reload.merge_and_unload()
        merged_model.save_pretrained(str(merged_path), safe_serialization=True)
        tokenizer.save_pretrained(str(merged_path))
    else:
        merged_model = model.merge_and_unload()
        merged_model.save_pretrained(str(merged_path), safe_serialization=True)
        tokenizer.save_pretrained(str(merged_path))

    print("[SUCCESS] Merged HuggingFace weights ready for llama.cpp GGUF conversion!")

    # 7. Exact llama.cpp Conversion + Importance-Matrix Quantization + Upload Commands
    print("\n" + "=" * 74)
    print(" Lab-Box -> llama.cpp Importance-Matrix Quantization & Upload Back")
    print("=" * 74)
    print(f"  1. Convert merged weights to FP16 GGUF:")
    print(f"     python3 llama.cpp/convert_hf_to_gguf.py {merged_path} --outfile omni-coder-f16.gguf --outtype f16")
    print("")
    print(f"  2. Compute Importance Matrix on LLVM-IR ChatML activations:")
    print(f"     ./llama.cpp/build/bin/llama-imatrix -m omni-coder-f16.gguf -f {imatrix_calib_path} -o omni.imatrix -ngl 99")
    print("")
    print(f"  3. Quantize to Q5_K_M (recommended ~1.13 GB) or Q4_K_M (~0.98 GB):")
    print(f"     ./llama.cpp/build/bin/llama-quantize --imatrix omni.imatrix omni-coder-f16.gguf omni-coder.gguf Q5_K_M")
    print("")
    print(f"  4. Stream omni-coder.gguf straight back to Surface (`models/omni-coder.gguf`):")
    print(f"     curl -T omni-coder.gguf {args.upload_url}")
    print("=" * 74 + "\n")


if __name__ == "__main__":
    main()
