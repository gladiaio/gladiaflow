# OpenPII on Apple MLX

Local XLM-RoBERTa-Large token classifier (OpenPII) for GladiaFlow screen-context vocabulary.

## Layout

- `model.py` — MLX XLM-R + classification head
- `infer.py` — span decode + vocab allowlist
- `serve.py` — `127.0.0.1` HTTP sidecar (`/health`, `/ner/terms`, `/ner/text`)

## Model path

Default: repo `.models/openpii-xlmrL2-hf/` (HF `model.safetensors` + `tokenizer.json` + `config.json`).

Override: `GLADIAFLOW_NER_MLX_MODEL_DIR` or `OPENPII_MLX_PORT` (default `8765`).

## Manual run

```bash
python3 tools/openpii_mlx/serve.py --model-dir .models/openpii-xlmrL2-hf
curl -s http://127.0.0.1:8765/health
curl -s -X POST http://127.0.0.1:8765/ner/terms \
  -H 'content-type: application/json' \
  -d '{"text":"Jean-Louis from GladiaFlow emailed support@gladia.io in Paris"}'
```

GladiaFlow starts this sidecar automatically on macOS when weights are present, and falls back to ONNX/`ort` otherwise.

## Bench (M5 Pro)

- Load ~0.9 s
- Short text inference ~17 ms (faster than ONNX int8 CPU ~22 ms)
