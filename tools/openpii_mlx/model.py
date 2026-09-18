"""XLM-RoBERTa for token classification on Apple MLX (OpenPII)."""

from __future__ import annotations

import json
import math
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import mlx.core as mx
import mlx.nn as nn
from safetensors import safe_open


@dataclass
class ModelConfig:
    vocab_size: int = 250_002
    hidden_size: int = 1024
    num_hidden_layers: int = 24
    num_attention_heads: int = 16
    intermediate_size: int = 4096
    max_position_embeddings: int = 514
    type_vocab_size: int = 1
    layer_norm_eps: float = 1e-5
    pad_token_id: int = 1
    num_labels: int = 147

    @classmethod
    def from_hf_dir(cls, model_dir: Path) -> "ModelConfig":
        cfg = json.loads((model_dir / "config.json").read_text())
        id2label = cfg.get("id2label") or {}
        return cls(
            vocab_size=int(cfg["vocab_size"]),
            hidden_size=int(cfg["hidden_size"]),
            num_hidden_layers=int(cfg["num_hidden_layers"]),
            num_attention_heads=int(cfg["num_attention_heads"]),
            intermediate_size=int(cfg["intermediate_size"]),
            max_position_embeddings=int(cfg["max_position_embeddings"]),
            type_vocab_size=int(cfg.get("type_vocab_size", 1)),
            layer_norm_eps=float(cfg.get("layer_norm_eps", 1e-5)),
            pad_token_id=int(cfg.get("pad_token_id", 1)),
            num_labels=len(id2label) if id2label else int(cfg.get("num_labels") or 147),
        )


class XLMRobertaEmbeddings(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.word_embeddings = nn.Embedding(config.vocab_size, config.hidden_size)
        self.position_embeddings = nn.Embedding(
            config.max_position_embeddings, config.hidden_size
        )
        self.token_type_embeddings = nn.Embedding(
            config.type_vocab_size, config.hidden_size
        )
        self.LayerNorm = nn.LayerNorm(config.hidden_size, eps=config.layer_norm_eps)
        self.padding_idx = config.pad_token_id

    def create_position_ids_from_input_ids(self, input_ids: mx.array) -> mx.array:
        mask = (input_ids != self.padding_idx).astype(mx.int32)
        incremental = mx.cumsum(mask, axis=1) * mask
        return incremental + self.padding_idx

    def __call__(
        self,
        input_ids: mx.array,
        token_type_ids: Optional[mx.array] = None,
        position_ids: Optional[mx.array] = None,
    ) -> mx.array:
        if token_type_ids is None:
            token_type_ids = mx.zeros_like(input_ids)
        if position_ids is None:
            position_ids = self.create_position_ids_from_input_ids(input_ids)
        embeddings = (
            self.word_embeddings(input_ids)
            + self.token_type_embeddings(token_type_ids)
            + self.position_embeddings(position_ids)
        )
        return self.LayerNorm(embeddings)


class XLMRobertaSelfAttention(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.num_attention_heads = config.num_attention_heads
        self.attention_head_size = config.hidden_size // config.num_attention_heads
        self.all_head_size = self.num_attention_heads * self.attention_head_size
        self.query = nn.Linear(config.hidden_size, self.all_head_size)
        self.key = nn.Linear(config.hidden_size, self.all_head_size)
        self.value = nn.Linear(config.hidden_size, self.all_head_size)

    def transpose_for_scores(self, x: mx.array) -> mx.array:
        b, s, _ = x.shape
        x = x.reshape(b, s, self.num_attention_heads, self.attention_head_size)
        return x.transpose(0, 2, 1, 3)

    def __call__(self, hidden_states: mx.array, attention_mask: Optional[mx.array]):
        q = self.transpose_for_scores(self.query(hidden_states))
        k = self.transpose_for_scores(self.key(hidden_states))
        v = self.transpose_for_scores(self.value(hidden_states))
        scores = (q @ k.transpose(0, 1, 3, 2)) / math.sqrt(self.attention_head_size)
        if attention_mask is not None:
            scores = scores + attention_mask
        probs = mx.softmax(scores.astype(mx.float32), axis=-1).astype(scores.dtype)
        context = probs @ v
        context = context.transpose(0, 2, 1, 3)
        b, s, _, _ = context.shape
        return context.reshape(b, s, self.all_head_size)


class XLMRobertaSelfOutput(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.dense = nn.Linear(config.hidden_size, config.hidden_size)
        self.LayerNorm = nn.LayerNorm(config.hidden_size, eps=config.layer_norm_eps)

    def __call__(self, hidden_states: mx.array, input_tensor: mx.array) -> mx.array:
        return self.LayerNorm(self.dense(hidden_states) + input_tensor)


class XLMRobertaAttention(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.self = XLMRobertaSelfAttention(config)
        self.output = XLMRobertaSelfOutput(config)

    def __call__(self, hidden_states: mx.array, attention_mask: Optional[mx.array]):
        return self.output(self.self(hidden_states, attention_mask), hidden_states)


class XLMRobertaIntermediate(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.dense = nn.Linear(config.hidden_size, config.intermediate_size)

    def __call__(self, hidden_states: mx.array) -> mx.array:
        return nn.gelu(self.dense(hidden_states))


class XLMRobertaOutput(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.dense = nn.Linear(config.intermediate_size, config.hidden_size)
        self.LayerNorm = nn.LayerNorm(config.hidden_size, eps=config.layer_norm_eps)

    def __call__(self, hidden_states: mx.array, input_tensor: mx.array) -> mx.array:
        return self.LayerNorm(self.dense(hidden_states) + input_tensor)


class XLMRobertaLayer(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.attention = XLMRobertaAttention(config)
        self.intermediate = XLMRobertaIntermediate(config)
        self.output = XLMRobertaOutput(config)

    def __call__(self, hidden_states: mx.array, attention_mask: Optional[mx.array]):
        attention_output = self.attention(hidden_states, attention_mask)
        intermediate_output = self.intermediate(attention_output)
        return self.output(intermediate_output, attention_output)


class XLMRobertaEncoder(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.layer = [XLMRobertaLayer(config) for _ in range(config.num_hidden_layers)]

    def __call__(self, hidden_states: mx.array, attention_mask: Optional[mx.array]):
        for layer in self.layer:
            hidden_states = layer(hidden_states, attention_mask)
        return hidden_states


class XLMRobertaModel(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.config = config
        self.embeddings = XLMRobertaEmbeddings(config)
        self.encoder = XLMRobertaEncoder(config)

    def extended_attention_mask(self, attention_mask: mx.array) -> mx.array:
        # [B, S] -> [B, 1, 1, S], additive mask
        mask = attention_mask[:, None, None, :].astype(mx.float32)
        return (1.0 - mask) * -1e4

    def __call__(
        self,
        input_ids: mx.array,
        attention_mask: Optional[mx.array] = None,
        token_type_ids: Optional[mx.array] = None,
    ) -> mx.array:
        if attention_mask is None:
            attention_mask = mx.ones_like(input_ids)
        ext_mask = self.extended_attention_mask(attention_mask)
        hidden = self.embeddings(input_ids, token_type_ids)
        return self.encoder(hidden, ext_mask)


class XLMRobertaForTokenClassification(nn.Module):
    def __init__(self, config: ModelConfig):
        super().__init__()
        self.config = config
        self.roberta = XLMRobertaModel(config)
        self.classifier = nn.Linear(config.hidden_size, config.num_labels)

    def __call__(
        self,
        input_ids: mx.array,
        attention_mask: Optional[mx.array] = None,
        token_type_ids: Optional[mx.array] = None,
    ) -> mx.array:
        sequence_output = self.roberta(input_ids, attention_mask, token_type_ids)
        return self.classifier(sequence_output)


def _sanitize_hf_weights(weights: Dict[str, mx.array]) -> Dict[str, mx.array]:
    out: Dict[str, mx.array] = {}
    for key, value in weights.items():
        if key.endswith("position_ids"):
            continue
        # HF tree is already `roberta.*` + `classifier.*` — matches our Module.
        out[key] = value
    return out


def load_hf_safetensors(model_dir: Path) -> Dict[str, mx.array]:
    path = model_dir / "model.safetensors"
    weights: Dict[str, mx.array] = {}
    with safe_open(str(path), framework="np") as f:
        for key in f.keys():
            weights[key] = mx.array(f.get_tensor(key))
    return _sanitize_hf_weights(weights)


def build_model_from_hf_dir(model_dir: Path) -> Tuple[XLMRobertaForTokenClassification, List[str]]:
    model_dir = Path(model_dir)
    config = ModelConfig.from_hf_dir(model_dir)
    model = XLMRobertaForTokenClassification(config)
    weights = load_hf_safetensors(model_dir)
    model.load_weights(list(weights.items()), strict=True)
    mx.eval(model.parameters())
    id2label_raw = json.loads((model_dir / "config.json").read_text()).get("id2label")
    if not id2label_raw:
        # fallback file used by ONNX export
        id2label_path = model_dir / "id2label.json"
        if id2label_path.exists():
            id2label_raw = json.loads(id2label_path.read_text())
    id2label = [""] * config.num_labels
    for k, v in (id2label_raw or {}).items():
        id2label[int(k)] = v
    return model, id2label


def save_mlx_weights(model: XLMRobertaForTokenClassification, out_dir: Path) -> None:
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    flat = dict(tree_flatten(model.parameters()))
    # mx.savez wants valid python identifiers; keep dotted keys via safetensors instead.
    from safetensors.numpy import save_file

    numpy_weights = {k: v.astype(mx.float32) for k, v in flat.items()}
    # Convert mlx -> numpy
    import numpy as np

    np_weights = {k: np.array(v) for k, v in numpy_weights.items()}
    save_file(np_weights, str(out_dir / "model.safetensors"))
    (out_dir / "config.json").write_text(
        json.dumps(
            {
                "vocab_size": model.config.vocab_size,
                "hidden_size": model.config.hidden_size,
                "num_hidden_layers": model.config.num_hidden_layers,
                "num_attention_heads": model.config.num_attention_heads,
                "intermediate_size": model.config.intermediate_size,
                "max_position_embeddings": model.config.max_position_embeddings,
                "type_vocab_size": model.config.type_vocab_size,
                "layer_norm_eps": model.config.layer_norm_eps,
                "pad_token_id": model.config.pad_token_id,
                "num_labels": model.config.num_labels,
                "model_type": "xlm-roberta",
                "architectures": ["XLMRobertaForTokenClassification"],
            },
            indent=2,
        )
    )


def tree_flatten(tree, prefix: str = ""):
    """Flatten mlx nn parameter tree into (name, array) pairs."""
    if isinstance(tree, dict):
        for k, v in tree.items():
            name = f"{prefix}.{k}" if prefix else k
            yield from tree_flatten(v, name)
    elif isinstance(tree, (list, tuple)):
        for i, v in enumerate(tree):
            name = f"{prefix}.{i}" if prefix else str(i)
            yield from tree_flatten(v, name)
    else:
        yield prefix, tree
