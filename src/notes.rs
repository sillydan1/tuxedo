//! Compatibility helpers for the todo.txt-cli `note` add-on convention.
//!
//! A note is represented on a task by a `note:<filename>` token. The note file
//! lives in `${TODO_NOTES_DIR:-<todo-dir>/notes}` and defaults to a `.txt`
//! extension, matching the add-on's environment variable names.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::todo;

#[derive(Debug, Clone)]
pub struct NoteConfig {
    pub dir: PathBuf,
    pub tag: String,
    pub ext: String,
    template: String,
}

impl NoteConfig {
    pub fn from_env(todo_path: &Path) -> Self {
        let mut config = Self::default_for_path(todo_path);
        if let Some(dir) = std::env::var_os("TODO_NOTES_DIR").filter(|v| !v.is_empty()) {
            config.dir = PathBuf::from(dir);
        }
        if let Ok(tag) = std::env::var("TODO_NOTE_TAG")
            && !tag.is_empty()
        {
            config.tag = tag;
        }
        if let Ok(ext) = std::env::var("TODO_NOTE_EXT")
            && !ext.is_empty()
        {
            config.ext = ext;
        }
        if let Ok(template) = std::env::var("TODO_NOTE_TEMPLATE")
            && !template.is_empty()
        {
            config.template = template;
        }
        config
    }

    pub fn default_for_path(todo_path: &Path) -> Self {
        let dir = todo_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("notes");
        Self {
            dir,
            tag: "note".to_string(),
            ext: ".md".to_string(),
            template: "XXX".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreatedNote {
    pub name: String,
    pub path: PathBuf,
}

pub fn existing_note_name(raw: &str, tag: &str) -> Option<String> {
    raw.split_whitespace().find_map(|token| {
        let (key, value) = token.split_once(':')?;
        if key == tag && !value.is_empty() {
            Some(value.to_string())
        } else {
            None
        }
    })
}

pub fn path_for_note_name(config: &NoteConfig, name: &str) -> io::Result<PathBuf> {
    if !is_safe_note_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid note filename",
        ));
    }
    Ok(config.dir.join(name))
}

pub fn title_for_task(raw: &str) -> String {
    let title = todo::body_only(raw);
    if title.is_empty() {
        "note".to_string()
    } else {
        title
    }
}

pub fn create_note_file(config: &NoteConfig, title: &str) -> io::Result<CreatedNote> {
    if !is_safe_note_piece(&config.ext) || !is_safe_note_piece(&config.template) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid note filename template",
        ));
    }
    fs::create_dir_all(&config.dir)?;
    for attempt in 0..128 {
        let stem = render_template(&config.template, attempt);
        let name = format!("{stem}{}", config.ext);
        if !is_safe_note_name(&name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid note filename",
            ));
        }
        let path = config.dir.join(&name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = writeln!(file, "# {title}") {
                    let _ = fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(CreatedNote { name, path });
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate note filename",
    ))
}

pub fn is_valid_note_tag(tag: &str) -> bool {
    let mut chars = tag.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_safe_note_piece(piece: &str) -> bool {
    !piece.contains('/') && !piece.contains('\\') && !piece.contains('\0')
}

fn is_safe_note_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && is_safe_note_piece(name)
}

fn render_template(template: &str, attempt: u64) -> String {
    let source = randomish_chars(attempt);
    if !template.contains('X') {
        return format!("{template}{source}");
    }
    let mut chars = source.chars().cycle();
    template
        .chars()
        .map(|c| {
            if c == 'X' {
                chars.next().unwrap_or('0')
            } else {
                c
            }
        })
        .collect()
}

fn randomish_chars(attempt: u64) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    encode_base62(nanos ^ (counter << 64) ^ u128::from(attempt))
}

fn encode_base62(mut n: u128) -> String {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        let idx = (n % 62) as usize;
        out.push(ALPHABET[idx] as char);
        n /= 62;
    }
    out.iter().rev().collect()
}
