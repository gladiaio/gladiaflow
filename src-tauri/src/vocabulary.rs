use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

pub const DEFAULT_VOCAB_TERM: &str = "gladiaflow";
pub const DEFAULT_VOCAB_TERM_INTENSITY: f64 = 0.4;

fn default_vocab_intensity() -> f64 {
    0.5
}

pub fn default_custom_vocabulary() -> Vec<CustomVocabEntry> {
    vec![CustomVocabEntry {
        value: DEFAULT_VOCAB_TERM.to_string(),
        pronunciations: Some(vec!["gladioflow".to_string(), "gladiaflaw".to_string()]),
        language: None,
        intensity: DEFAULT_VOCAB_TERM_INTENSITY,
    }]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomVocabEntry {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pronunciations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default = "default_vocab_intensity")]
    pub intensity: f64,
}

/// Drop entries whose value is blank.
pub(crate) fn normalize_vocabulary(entries: Vec<CustomVocabEntry>) -> Vec<CustomVocabEntry> {
    entries
        .into_iter()
        .filter(|entry| !entry.value.trim().is_empty())
        .collect()
}

/// Duplicate terms without an explicit language across every configured language so
/// tech vocabulary is recognized regardless of which language is being spoken.
pub(crate) fn expand_vocabulary_for_languages(
    entries: Vec<CustomVocabEntry>,
    languages: &[String],
) -> Vec<CustomVocabEntry> {
    if languages.is_empty() {
        return entries;
    }

    let mut expanded = Vec::with_capacity(entries.len() * languages.len());
    for entry in entries {
        if entry.language.is_some() {
            expanded.push(entry);
            continue;
        }
        for lang in languages {
            expanded.push(CustomVocabEntry {
                language: Some(lang.clone()),
                ..entry.clone()
            });
        }
    }
    expanded
}

#[tauri::command]
pub async fn save_custom_vocabulary(vocabulary: Vec<CustomVocabEntry>) -> Result<(), String> {
    crate::config::save_custom_vocabulary(vocabulary)
}

#[tauri::command]
pub async fn get_custom_vocabulary() -> Result<Vec<CustomVocabEntry>, String> {
    crate::config::get_custom_vocabulary()
}

/// Opens a native "Save As" dialog and writes the provided CSV to the chosen path.
/// Returns `true` if the file was saved, `false` if the user cancelled the dialog.
#[tauri::command]
pub async fn export_vocabulary_csv(app: AppHandle, csv: String) -> Result<bool, String> {
    let mut builder = app
        .dialog()
        .file()
        .set_file_name("gladiaflow-vocabulary.csv")
        .add_filter("CSV", &["csv"]);
    if let Some(downloads) = dirs::download_dir() {
        builder = builder.set_directory(downloads);
    }
    let path = builder.blocking_save_file();

    match path {
        Some(file_path) => {
            let p = file_path.into_path().map_err(|e| e.to_string())?;
            std::fs::write(&p, csv).map_err(|e| e.to_string())?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Opens a native "Open" dialog and returns the chosen CSV file's contents.
/// Returns `None` if the user cancelled the dialog.
#[tauri::command]
pub async fn import_vocabulary_csv(app: AppHandle) -> Result<Option<String>, String> {
    let mut builder = app.dialog().file().add_filter("CSV", &["csv"]);
    if let Some(downloads) = dirs::download_dir() {
        builder = builder.set_directory(downloads);
    }
    match builder.blocking_pick_file() {
        Some(file_path) => {
            let p = file_path.into_path().map_err(|e| e.to_string())?;
            let csv = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            Ok(Some(csv))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_custom_vocabulary_contains_gladiaflow() {
        let vocab = default_custom_vocabulary();
        assert_eq!(vocab.len(), 1);
        assert_eq!(vocab[0].value, DEFAULT_VOCAB_TERM);
        assert_eq!(vocab[0].intensity, DEFAULT_VOCAB_TERM_INTENSITY);
    }
}
