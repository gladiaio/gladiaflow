"""OpenPII NER inference helpers on MLX."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import mlx.core as mx
import numpy as np
from tokenizers import Tokenizer

from model import XLMRobertaForTokenClassification, build_model_from_hf_dir

VOCAB_LABELS = {
    "NAME",
    "NAME_GIVEN",
    "NAME_FAMILY",
    "NAME_MEDICAL_PROFESSIONAL",
    "ORGANIZATION",
    "ORGANIZATION_MEDICAL_FACILITY",
    "PRODUCT",
    "LOCATION_CITY",
    "LOCATION_COUNTRY",
    "LOCATION_STATE",
    "LOCATION",
    "EMAIL_ADDRESS",
    "USERNAME",
    "EVENT",
}

SKIP_LABELS = {
    "PASSWORD",
    "SSN",
    "CREDIT_CARD",
    "CVV",
    "ACCOUNT_NUMBER",
    "BANK_ACCOUNT",
    "ROUTING_NUMBER",
    "DRIVER_LICENSE",
    "PASSPORT_NUMBER",
    "HEALTHCARE_NUMBER",
    "VEHICLE_ID",
}


def base_label(label: str) -> str:
    for prefix in ("B-", "I-", "B_", "I_"):
        if label.startswith(prefix):
            return label[len(prefix) :]
    return label


class OpenPiiMlxNer:
    def __init__(self, model_dir: Path):
        self.model_dir = Path(model_dir)
        self.model, self.id2label = build_model_from_hf_dir(self.model_dir)
        self.model.eval()
        tok_path = self.model_dir / "tokenizer.json"
        self.tokenizer = Tokenizer.from_file(str(tok_path))
        self.o_id = next((i for i, l in enumerate(self.id2label) if l == "O"), 0)
        self.threshold = 0.25
        self.chunk_chars = 1200

    def extract_terms(self, text: str) -> List[str]:
        spans = self.tag_text(text)
        out: List[str] = []
        seen = set()
        for start, end, label, _score in spans:
            if label in SKIP_LABELS or label not in VOCAB_LABELS:
                continue
            value = text[start:end].strip()
            if len(value) < 2:
                continue
            key = value.lower()
            if key in seen:
                continue
            seen.add(key)
            out.append(value)
        return out

    def tag_text(self, text: str) -> List[Tuple[int, int, str, float]]:
        spans: List[Tuple[int, int, str, float]] = []
        for base, chunk in self._chunk_text(text):
            for s, e, label, score in self._tag_chunk(chunk):
                spans.append((base + s, base + e, label, score))
        return spans

    def _chunk_text(self, text: str) -> List[Tuple[int, str]]:
        out: List[Tuple[int, str]] = []
        base = 0
        while base < len(text):
            end = min(base + self.chunk_chars, len(text))
            if end < len(text):
                ws = text.rfind(" ", base, end)
                if ws > base:
                    end = ws
            out.append((base, text[base:end]))
            base = max(end, base + 1)
        return out

    def _tag_chunk(self, chunk: str) -> List[Tuple[int, int, str, float]]:
        enc = self.tokenizer.encode(chunk, add_special_tokens=True)
        ids = np.array([enc.ids], dtype=np.int32)
        mask = np.array([enc.attention_mask], dtype=np.int32)
        offsets = enc.offsets
        logits = self.model(mx.array(ids), mx.array(mask))
        mx.eval(logits)
        row = np.array(logits[0])  # [seq, labels]
        return self._decode_row(row, offsets)

    def _decode_row(
        self, logits: np.ndarray, offsets: List[Tuple[int, int]]
    ) -> List[Tuple[int, int, str, float]]:
        spans: List[Tuple[int, int, str, float]] = []
        cur = None  # start, end, base, max_prob

        def flush():
            nonlocal cur
            if cur is not None:
                spans.append((cur[0], cur[1], cur[2], float(cur[3])))
                cur = None

        seq = min(len(offsets), logits.shape[0])
        for t in range(seq):
            a, b = offsets[t]
            if a == b:
                continue
            row = logits[t]
            maxv = row.max()
            exp = np.exp(row - maxv)
            probs = exp / exp.sum()
            # best non-O
            best_id = self.o_id
            best_p = -1.0
            for i, p in enumerate(probs):
                if i == self.o_id:
                    continue
                if p > best_p:
                    best_p = float(p)
                    best_id = i
            label = self.id2label[best_id] if best_p >= self.threshold else "O"
            if label == "O":
                flush()
                continue
            base = base_label(label)
            if cur and cur[2] == base:
                cur = (cur[0], b, base, max(cur[3], best_p))
            else:
                flush()
                cur = (a, b, base, best_p)
        flush()
        return spans


def resolve_default_model_dir() -> Optional[Path]:
    root = Path(__file__).resolve().parents[2]
    cand = root / ".models" / "openpii-xlmrL2-hf"
    return cand if (cand / "model.safetensors").exists() else None
