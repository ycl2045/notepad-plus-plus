#![allow(dead_code)]
//! Extension → syntax mapping and language menu data.
//!
//! OWNED BY: U1 (contract) → U5 (≥20 languages, syntax fallback rules).

use std::collections::BTreeMap;

/// (display name, syntect syntax name / "Plain Text", extensions)
pub struct LanguageDef {
    pub display: &'static str,
    pub syntax: &'static str,
    pub exts: &'static [&'static str],
}

pub const LANGUAGES: &[LanguageDef] = &[
    LanguageDef { display: "Rust", syntax: "Rust", exts: &["rs"] },
    LanguageDef { display: "C", syntax: "C", exts: &["c", "h"] },
    LanguageDef { display: "C++", syntax: "C++", exts: &["cpp", "cxx", "cc", "hpp", "hxx"] },
    LanguageDef { display: "Python", syntax: "Python", exts: &["py", "pyw"] },
    LanguageDef { display: "JavaScript", syntax: "JavaScript", exts: &["js", "mjs", "cjs"] },
    LanguageDef { display: "TypeScript", syntax: "TypeScript", exts: &["ts", "tsx"] },
    LanguageDef { display: "JSON", syntax: "JSON", exts: &["json", "jsonc"] },
    LanguageDef { display: "HTML", syntax: "HTML", exts: &["html", "htm", "xhtml"] },
    LanguageDef { display: "CSS", syntax: "CSS", exts: &["css", "scss", "sass", "less"] },
    LanguageDef { display: "XML", syntax: "XML", exts: &["xml", "svg", "ui", "plist"] },
    LanguageDef { display: "Markdown", syntax: "Markdown", exts: &["md", "markdown"] },
    LanguageDef { display: "Shell", syntax: "Shell (Bash)", exts: &["sh", "bash", "zsh"] },
    LanguageDef { display: "Python (Jinja)", syntax: "HTML (Jinja2)", exts: &["jinja", "j2"] },
    LanguageDef { display: "Go", syntax: "Go", exts: &["go"] },
    LanguageDef { display: "Java", syntax: "Java", exts: &["java"] },
    LanguageDef { display: "Kotlin", syntax: "Kotlin", exts: &["kt", "kts"] },
    LanguageDef { display: "Swift", syntax: "Swift", exts: &["swift"] },
    LanguageDef { display: "Ruby", syntax: "Ruby", exts: &["rb", "rake", "gemspec"] },
    LanguageDef { display: "PHP", syntax: "PHP", exts: &["php"] },
    LanguageDef { display: "SQL", syntax: "SQL", exts: &["sql"] },
    LanguageDef { display: "YAML", syntax: "YAML", exts: &["yaml", "yml"] },
    LanguageDef { display: "TOML", syntax: "TOML", exts: &["toml", "lock"] },
    LanguageDef { display: "INI", syntax: "INI", exts: &["ini", "cfg", "conf"] },
    LanguageDef { display: "Makefile", syntax: "Makefile", exts: &["mak", "mk"] },
    LanguageDef { display: "CMake", syntax: "CMake", exts: &["cmake"] },
    LanguageDef { display: "Diff", syntax: "Diff", exts: &["diff", "patch"] },
    LanguageDef { display: "Lua", syntax: "Lua", exts: &["lua"] },
    LanguageDef { display: "Perl", syntax: "Perl", exts: &["pl", "pm"] },
    LanguageDef { display: "PowerShell", syntax: "PowerShell", exts: &["ps1", "psm1"] },
    LanguageDef { display: "Batchfile", syntax: "Batch File", exts: &["bat", "cmd"] },
];

pub fn ext_to_syntax(path: &std::path::Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name == "makefile" || name == "gnumakefile" {
        return "Makefile".to_string();
    }
    if name == "dockerfile" {
        return "Dockerfile".to_string();
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    for lang in LANGUAGES {
        if lang.exts.contains(&ext.as_str()) {
            return lang.syntax.to_string();
        }
    }
    "Plain Text".to_string()
}

/// Sorted map of display name → syntax name, for building the language menu.
pub fn menu_entries() -> BTreeMap<&'static str, &'static str> {
    let mut out = BTreeMap::new();
    out.insert("Plain Text", "Plain Text");
    for lang in LANGUAGES {
        out.insert(lang.display, lang.syntax);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn maps_known_extensions() {
        assert_eq!(ext_to_syntax(&PathBuf::from("a.rs")), "Rust");
        assert_eq!(ext_to_syntax(&PathBuf::from("a.PY")), "Python");
        assert_eq!(ext_to_syntax(&PathBuf::from("a.json")), "JSON");
        assert_eq!(ext_to_syntax(&PathBuf::from("Makefile")), "Makefile");
        assert_eq!(ext_to_syntax(&PathBuf::from("Dockerfile")), "Dockerfile");
    }

    #[test]
    fn unknown_extensions_fall_back_to_plain_text() {
        assert_eq!(ext_to_syntax(&PathBuf::from("a.zzzz")), "Plain Text");
        assert_eq!(ext_to_syntax(&PathBuf::from("noext")), "Plain Text");
    }

    #[test]
    fn has_at_least_twenty_languages() {
        assert!(LANGUAGES.len() >= 20, "only {} languages", LANGUAGES.len());
    }
}
