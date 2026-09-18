"""Convert HF OpenPII weights into a flat MLX-friendly safetensors (optional).

Loading HF weights directly via model.build_model_from_hf_dir is already supported;
this script is useful to copy tokenizer/config next to a sanitized weight file.
"""

from __future__ import annotations

import argparse
import shutil
from pathlib import Path

from model import build_model_from_hf_dir, save_mlx_weights


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--src", required=True, type=Path)
    parser.add_argument("--dst", required=True, type=Path)
    args = parser.parse_args()
    model, id2label = build_model_from_hf_dir(args.src)
    save_mlx_weights(model, args.dst)
    for name in ("tokenizer.json", "tokenizer_config.json", "config.json"):
        src = args.src / name
        if src.exists():
            shutil.copy2(src, args.dst / name)
    # ensure id2label present for non-HF consumers
    import json

    (args.dst / "id2label.json").write_text(
        json.dumps({str(i): lab for i, lab in enumerate(id2label)}, indent=2)
    )
    print(f"wrote MLX bundle to {args.dst}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
