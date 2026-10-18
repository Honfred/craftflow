//! New, open, save and save-as.

use std::path::PathBuf;

use graph::Schema;

use crate::state::AppState;

const FILTER_NAME: &str = "Craftflow schema";
const FILTER_EXT: &str = "json";

pub fn new_schema(state: &mut AppState) {
    state.load_schema(Schema::default(), None);
    state.status = Some("new schema".to_owned());
}

pub fn open(state: &mut AppState) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(FILTER_NAME, &[FILTER_EXT])
        .pick_file()
    else {
        return;
    };
    open_path(state, path);
}

pub fn open_path(state: &mut AppState, path: PathBuf) {
    match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| Schema::from_json(&text).map_err(|e| e.to_string()))
    {
        Ok(schema) => {
            let name = path.display().to_string();
            state.load_schema(schema, Some(path));
            state.status = Some(format!("opened {name}"));
        }
        Err(error) => state.status = Some(format!("could not open: {error}")),
    }
}

/// Saves to the current path, asking for one the first time.
pub fn save(state: &mut AppState) {
    match state.path.clone() {
        Some(path) => write(state, path),
        None => save_as(state),
    }
}

pub fn save_as(state: &mut AppState) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(FILTER_NAME, &[FILTER_EXT])
        .set_file_name("schema.json")
        .save_file()
    else {
        return;
    };
    write(state, path);
}

fn write(state: &mut AppState, path: PathBuf) {
    let text = match state.editor.schema().to_json_pretty() {
        Ok(text) => text,
        Err(error) => {
            state.status = Some(format!("could not serialise: {error}"));
            return;
        }
    };
    match std::fs::write(&path, text) {
        Ok(()) => {
            state.editor.mark_saved();
            state.status = Some(format!("saved {}", path.display()));
            state.path = Some(path);
        }
        Err(error) => state.status = Some(format!("could not save: {error}")),
    }
}
