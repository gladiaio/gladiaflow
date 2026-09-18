//! Optional on-device OpenPII NER (ONNX / ort) for screen-context vocabulary.
//!
//! Model is loaded lazily from `GLADIAFLOW_NER_MODEL_DIR` or well-known local
//! paths. When the model is missing or still warming up, harvest falls back to
//! the capitalisation heuristic alone.

use ndarray::Array2;
use ort::session::Session;
use ort::value::Value;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tokenizers::Tokenizer;

/// Labels that are useful as dictation vocabulary (skip pure secrets).
const VOCAB_LABELS: &[&str] = &[
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
];

const SKIP_LABELS: &[&str] = &[
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
];

static ENGINE: OnceLock<Option<NerEngine>> = OnceLock::new();

fn base_label(lbl: &str) -> &str {
    lbl.strip_prefix("B-")
        .or_else(|| lbl.strip_prefix("I-"))
        .or_else(|| lbl.strip_prefix("B_"))
        .or_else(|| lbl.strip_prefix("I_"))
        .unwrap_or(lbl)
}

fn label_allowed(base: &str) -> bool {
    if SKIP_LABELS.iter().any(|s| *s == base) {
        return false;
    }
    VOCAB_LABELS.iter().any(|s| *s == base)
}

/// Resolve the OpenPII ONNX model directory, if present on disk.
pub fn resolve_model_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("GLADIAFLOW_NER_MODEL_DIR") {
        let path = PathBuf::from(dir);
        if looks_like_model_dir(&path) {
            return Some(path);
        }
    }

    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".models/openpii-xlmrL2-int8"));
        candidates.push(cwd.join("../.models/openpii-xlmrL2-int8"));
        candidates.push(cwd.join("../../.models/openpii-xlmrL2-int8"));
    }
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("gladiaflow/models/openpii-xlmrL2-int8"));
    }
    // Dev checkout: models live next to the Tauri project root.
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.models/openpii-xlmrL2-int8"));

    candidates.into_iter().find(|p| looks_like_model_dir(p))
}

fn looks_like_model_dir(path: &Path) -> bool {
    path.join("tokenizer.json").is_file()
        && path.join("id2label.json").is_file()
        && (path.join("model.onnx").is_file() || path.join("model.quant.onnx").is_file())
}

struct NerEngine {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    id2label: Vec<String>,
    o_id: usize,
    threshold: f32,
    chunk_chars: usize,
}

impl NerEngine {
    fn from_dir(dir: &Path) -> Result<Self, String> {
        let model_path = {
            let fp = dir.join("model.onnx");
            if fp.exists() {
                fp
            } else {
                dir.join("model.quant.onnx")
            }
        };
        let session = Session::builder()
            .map_err(|e| e.to_string())?
            .commit_from_file(&model_path)
            .map_err(|e| e.to_string())?;
        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| e.to_string())?;
        let raw: std::collections::HashMap<String, String> = serde_json::from_reader(
            std::fs::File::open(dir.join("id2label.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let n = raw.len();
        let mut id2label = vec![String::new(); n];
        for (k, v) in raw {
            let i: usize = k.parse().map_err(|e| format!("bad id2label key: {e}"))?;
            if i < n {
                id2label[i] = v;
            }
        }
        let o_id = id2label.iter().position(|l| l == "O").unwrap_or(0);
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            id2label,
            o_id,
            threshold: 0.25,
            chunk_chars: 1_200,
        })
    }

    fn extract_terms(&self, text: &str) -> Vec<String> {
        let mut terms = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (base, chunk) in self.chunk_text(text) {
            for (s, e, label, _score) in self.tag_chunk(chunk) {
                if !label_allowed(&label) {
                    continue;
                }
                let abs_s = base + s;
                let abs_e = base + e;
                if abs_e > text.len() || abs_s >= abs_e || !text.is_char_boundary(abs_s) || !text.is_char_boundary(abs_e)
                {
                    continue;
                }
                let value = text[abs_s..abs_e].trim();
                if value.chars().count() < 2 {
                    continue;
                }
                let key = value.to_lowercase();
                if seen.insert(key) {
                    terms.push(value.to_string());
                }
            }
        }
        terms
    }

    fn chunk_text<'a>(&self, text: &'a str) -> Vec<(usize, &'a str)> {
        let mut out = Vec::new();
        let mut base = 0usize;
        while base < text.len() {
            let mut end = (base + self.chunk_chars).min(text.len());
            while end < text.len() && !text.is_char_boundary(end) {
                end += 1;
            }
            if end < text.len() {
                if let Some(ws) = text[base..end].rfind(char::is_whitespace) {
                    if ws > 0 {
                        end = base + ws;
                    }
                }
            }
            out.push((base, &text[base..end]));
            base = end.max(base + 1);
        }
        out
    }

    fn tag_chunk(&self, chunk: &str) -> Vec<(usize, usize, String, f64)> {
        let Ok(encoding) = self.tokenizer.encode(chunk, true) else {
            return Vec::new();
        };
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        let offsets = encoding.get_offsets().to_vec();
        let seq = ids.len();
        if seq == 0 {
            return Vec::new();
        }
        let id_arr = match Array2::from_shape_vec((1, seq), ids) {
            Ok(a) => a,
            Err(_) => return Vec::new(),
        };
        let mask_arr = match Array2::from_shape_vec((1, seq), mask) {
            Ok(a) => a,
            Err(_) => return Vec::new(),
        };
        let (id_v, mask_v) = match (Value::from_array(id_arr), Value::from_array(mask_arr)) {
            (Ok(a), Ok(m)) => (a, m),
            _ => return Vec::new(),
        };
        let Ok(mut sess) = self.session.lock() else {
            return Vec::new();
        };
        let Ok(outputs) = sess.run(ort::inputs![
            "input_ids" => id_v,
            "attention_mask" => mask_v,
        ]) else {
            return Vec::new();
        };
        let Ok((shape, data)) = outputs["logits"].try_extract_tensor::<f32>() else {
            return Vec::new();
        };
        let num_labels = *shape.last().unwrap_or(&0) as usize;
        if num_labels == 0 {
            return Vec::new();
        }
        self.decode_row(data, 0, seq, num_labels, &offsets, seq)
    }

    fn decode_row(
        &self,
        data: &[f32],
        row: usize,
        maxlen: usize,
        num_labels: usize,
        offsets: &[(usize, usize)],
        seq: usize,
    ) -> Vec<(usize, usize, String, f64)> {
        let mut spans: Vec<(usize, usize, String, f64)> = Vec::new();
        let mut cur: Option<(usize, usize, String, f32)> = None;
        let flush = |cur: &mut Option<(usize, usize, String, f32)>,
                     spans: &mut Vec<(usize, usize, String, f64)>| {
            if let Some((s, e, base, p)) = cur.take() {
                spans.push((s, e, base, p as f64));
            }
        };
        let upto = seq.min(offsets.len());
        for t in 0..upto {
            let (a, b) = offsets[t];
            if a == b {
                continue;
            }
            let start = (row * maxlen + t) * num_labels;
            let rowl = &data[start..start + num_labels];
            let maxv = rowl.iter().cloned().fold(f32::MIN, f32::max);
            let sum: f32 = rowl.iter().map(|&x| (x - maxv).exp()).sum();
            let mut best_id = self.o_id;
            let mut best_p = -1.0f32;
            for (i, &logit) in rowl.iter().enumerate() {
                if i == self.o_id {
                    continue;
                }
                let p = (logit - maxv).exp() / sum;
                if p > best_p {
                    best_p = p;
                    best_id = i;
                }
            }
            let label = if best_p >= self.threshold {
                self.id2label
                    .get(best_id)
                    .map(String::as_str)
                    .unwrap_or("O")
            } else {
                "O"
            };
            if label == "O" {
                flush(&mut cur, &mut spans);
                continue;
            }
            let base = base_label(label).to_string();
            match &mut cur {
                Some((_, e, cbase, p)) if *cbase == base => {
                    *e = b;
                    *p = p.max(best_p);
                }
                _ => {
                    flush(&mut cur, &mut spans);
                    cur = Some((a, b, base, best_p));
                }
            }
        }
        flush(&mut cur, &mut spans);
        spans
    }
}

/// Load the NER engine once (or record that it is unavailable). Safe to call from a background thread.
pub fn warmup() {
    let _ = ENGINE.get_or_init(|| match resolve_model_dir() {
        Some(dir) => match NerEngine::from_dir(&dir) {
            Ok(engine) => {
                log::info!(
                    "[screen_context] OpenPII NER ready from {}",
                    dir.display()
                );
                Some(engine)
            }
            Err(error) => {
                log::warn!("[screen_context] OpenPII NER load failed: {error}");
                None
            }
        },
        None => {
            log::info!("[screen_context] OpenPII NER model not found; heuristic filter only");
            None
        }
    });
}

/// True when a warmed NER engine is available.
pub fn is_ready() -> bool {
    matches!(ENGINE.get(), Some(Some(_)))
}

/// Extract vocabulary-relevant entity strings. Empty when NER is not ready.
pub fn extract_entity_terms(text: &str) -> Vec<String> {
    match ENGINE.get() {
        Some(Some(engine)) => engine.extract_terms(text),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_allowlist_skips_secrets() {
        assert!(label_allowed("ORGANIZATION"));
        assert!(label_allowed("EMAIL_ADDRESS"));
        assert!(!label_allowed("PASSWORD"));
        assert!(!label_allowed("SSN"));
    }

    #[test]
    fn smoke_ner_when_model_present() {
        let Some(dir) = resolve_model_dir() else {
            eprintln!("skip: OpenPII model not on disk");
            return;
        };
        let engine = NerEngine::from_dir(&dir).expect("load NER");
        let terms = engine.extract_terms(
            "Jean-Louis Queguiner from GladiaFlow emailed support@gladia.io in Paris",
        );
        assert!(
            terms.iter().any(|t| t.contains("GladiaFlow") || t.contains("Jean")),
            "terms={terms:?}"
        );
        assert!(
            terms.iter().any(|t| t.contains('@')),
            "expected email in {terms:?}"
        );
    }
}
