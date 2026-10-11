/* crates/mc-core/src/i18n.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Translation helpers with the same names and xgettext keywords as the GTK application, backed
//! by a pure Rust `.mo` reader instead of libintl.

use std::path::Path;
use std::sync::OnceLock;

use gettext::Catalog;

pub const GETTEXT_PACKAGE: &str = "taskmanager";

static CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();

/// Candidate locale names for the current environment, most specific first.
pub fn requested_languages() -> Vec<String> {
    fn env(name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }

    let mut raw = Vec::new();
    // LANGUAGE is only honored when a locale is configured, like glibc does
    let locale = env("LC_ALL")
        .or_else(|| env("LC_MESSAGES"))
        .or_else(|| env("LANG"));
    if let Some(language) = env("LANGUAGE").filter(|_| locale.is_some()) {
        raw.extend(language.split(':').map(str::to_owned));
    }
    raw.extend(locale);

    let mut result = Vec::new();
    for name in raw {
        if name == "C" || name == "POSIX" || name.starts_with("C.") {
            continue;
        }

        let (base, modifier) = match name.split_once('@') {
            Some((b, m)) => (b, Some(m)),
            None => (name.as_str(), None),
        };
        let base = base.split('.').next().unwrap_or_default();
        let lang = base.split('_').next().unwrap_or_default();

        let mut push = |candidate: String| {
            if !candidate.is_empty() && !result.contains(&candidate) {
                result.push(candidate);
            }
        };

        if let Some(modifier) = modifier {
            push(format!("{base}@{modifier}"));
            push(format!("{lang}@{modifier}"));
        }
        push(base.to_owned());
        push(lang.to_owned());
    }

    result
}

fn load_catalog(locale_dir: &Path, domain: &str) -> Option<Catalog> {
    for language in requested_languages() {
        let path = locale_dir
            .join(&language)
            .join("LC_MESSAGES")
            .join(format!("{domain}.mo"));

        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };

        match Catalog::parse(std::io::BufReader::new(file)) {
            Ok(catalog) => {
                log::debug!(target: "TaskManager::I18n", "Loaded {}", path.display());
                return Some(catalog);
            }
            Err(e) => {
                log::warn!(target: "TaskManager::I18n", "Failed to parse {}: {e}", path.display());
            }
        }
    }

    None
}

/// Loads the translation catalog. Must be called once before any UI text is produced; later
/// calls are ignored. Without a catalog all strings are returned untranslated.
pub fn init(locale_dir: &Path) {
    CATALOG.get_or_init(|| load_catalog(locale_dir, GETTEXT_PACKAGE));
}

fn catalog() -> Option<&'static Catalog> {
    CATALOG.get().and_then(Option::as_ref)
}

fn gettext(msg: &str) -> String {
    match catalog() {
        Some(c) => c.gettext(msg).to_owned(),
        None => msg.to_owned(),
    }
}

fn ngettext(single: &str, multiple: &str, n: u32) -> String {
    match catalog() {
        Some(c) => c.ngettext(single, multiple, n as u64).to_owned(),
        None => if n == 1 { single } else { multiple }.to_owned(),
    }
}

fn pgettext(ctx: &str, msg: &str) -> String {
    match catalog() {
        Some(c) => c.pgettext(ctx, msg).to_owned(),
        None => msg.to_owned(),
    }
}

fn freplace(input: String, args: &[&str]) -> String {
    let mut parts = input.split("{}");
    let mut output = parts.next().unwrap_or_default().to_string();
    for (p, a) in parts.zip(args.iter()) {
        output += &((*a).to_string() + p);
    }
    output
}

/// Marks a literal for extraction by xgettext (keyword `n_`) without translating it; the
/// value is translated later with `i18n(...)`.
pub const fn n_(msgid: &'static str) -> &'static str {
    msgid
}

// Simple translations functions

pub fn i18n(format: &str) -> String {
    gettext(format)
}

pub fn i18n_f(format: &str, args: &[&str]) -> String {
    freplace(gettext(format), args)
}

// Singular and plural translations functions

pub fn ni18n_f(single: &str, multiple: &str, number: u32, args: &[&str]) -> String {
    freplace(ngettext(single, multiple, number), args)
}

// Translations with context functions

pub fn pi18n(ctx: &str, format: &str) -> String {
    pgettext(ctx, format)
}

/// Removes GTK mnemonic underscores (`_Quit` -> `Quit`); egui has no mnemonics.
pub fn strip_mnemonic(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_replacement() {
        assert_eq!(freplace("{} of {}".into(), &["1", "2"]), "1 of 2");
        assert_eq!(freplace("{} and {}".into(), &["a"]), "a and ");
        assert_eq!(freplace("no args".into(), &["x"]), "no args");
    }

    #[test]
    fn untranslated_plural_selection() {
        assert_eq!(ni18n_f("{} day", "{} days", 1, &["1"]), "1 day");
        assert_eq!(ni18n_f("{} day", "{} days", 3, &["3"]), "3 days");
    }

    #[test]
    fn mnemonics_are_stripped() {
        assert_eq!(strip_mnemonic("_Quit"), "Quit");
        assert_eq!(strip_mnemonic("Force _Stop"), "Force Stop");
        assert_eq!(strip_mnemonic("a__b"), "a_b");
    }
}
