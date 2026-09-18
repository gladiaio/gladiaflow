#!/usr/bin/env python3
"""Local OpenPII MLX NER sidecar for GladiaFlow (127.0.0.1 only)."""

from __future__ import annotations

import argparse
import json
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Optional
from urllib.parse import urlparse

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from infer import OpenPiiMlxNer, resolve_default_model_dir  # noqa: E402


class Handler(BaseHTTPRequestHandler):
    ner: Optional[OpenPiiMlxNer] = None

    def log_message(self, fmt: str, *args) -> None:
        sys.stderr.write("[openpii-mlx] " + (fmt % args) + "\n")

    def _read_json(self):
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        return json.loads(raw.decode("utf-8"))

    def _write_json(self, code: int, payload) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        path = urlparse(self.path).path
        if path in ("/health", "/"):
            self._write_json(
                200,
                {
                    "ok": True,
                    "engine": "mlx",
                    "ready": Handler.ner is not None,
                },
            )
            return
        self._write_json(404, {"error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        path = urlparse(self.path).path
        if Handler.ner is None:
            self._write_json(503, {"error": "model not ready"})
            return
        if path == "/ner/terms":
            data = self._read_json()
            text = data.get("text") or ""
            if not isinstance(text, str):
                self._write_json(400, {"error": "text must be a string"})
                return
            terms = Handler.ner.extract_terms(text)
            self._write_json(200, {"terms": terms, "engine": "mlx"})
            return
        if path == "/ner/text":
            data = self._read_json()
            texts = data.get("text")
            if isinstance(texts, str):
                texts = [texts]
            if not isinstance(texts, list):
                self._write_json(400, {"error": "text must be string or list"})
                return
            out = []
            for text in texts:
                spans = [
                    {
                        "start": s,
                        "end": e,
                        "label": label,
                        "score": score,
                        "text": text[s:e],
                    }
                    for s, e, label, score in Handler.ner.tag_text(text)
                ]
                out.append({"entities": spans})
            self._write_json(200, out)
            return
        self._write_json(404, {"error": "not found"})


def main() -> int:
    parser = argparse.ArgumentParser(description="OpenPII MLX NER sidecar")
    parser.add_argument(
        "--model-dir",
        default=os.environ.get("GLADIAFLOW_NER_MLX_MODEL_DIR")
        or os.environ.get("GLADIAFLOW_NER_MODEL_DIR"),
    )
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=int(os.environ.get("OPENPII_MLX_PORT", "8765")))
    args = parser.parse_args()

    model_dir = Path(args.model_dir) if args.model_dir else resolve_default_model_dir()
    if model_dir is None or not (model_dir / "model.safetensors").exists():
        print("openpii-mlx: model dir not found", file=sys.stderr)
        return 1

    print(f"openpii-mlx: loading {model_dir}", file=sys.stderr)
    Handler.ner = OpenPiiMlxNer(model_dir)
    # Warm first inference so the first GladiaFlow press is cold-start free.
    Handler.ner.extract_terms("GladiaFlow warmup support@gladia.io Paris")
    print(f"openpii-mlx: ready on http://{args.host}:{args.port}", file=sys.stderr)

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
