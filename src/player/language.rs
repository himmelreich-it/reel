/// Languages offered as audio and subtitle preferences.
///
/// Config stores the ISO 639-1 code. Media files tag tracks with ISO 639-1,
/// ISO 639-2/T or ISO 639-2/B codes (sometimes with a region suffix), so each
/// entry lists every code a matching track may carry.
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub track_codes: &'static [&'static str],
}

pub const LANGUAGES: &[Language] = &[
    lang("ar", "Arabic", &["ara"]),
    lang("zh", "Chinese", &["zho", "chi"]),
    lang("cs", "Czech", &["ces", "cze"]),
    lang("da", "Danish", &["dan"]),
    lang("nl", "Dutch", &["nld", "dut"]),
    lang("en", "English", &["eng"]),
    lang("fi", "Finnish", &["fin"]),
    lang("fr", "French", &["fra", "fre"]),
    lang("de", "German", &["deu", "ger"]),
    lang("el", "Greek", &["ell", "gre"]),
    lang("he", "Hebrew", &["heb"]),
    lang("hi", "Hindi", &["hin"]),
    lang("hu", "Hungarian", &["hun"]),
    lang("it", "Italian", &["ita"]),
    lang("ja", "Japanese", &["jpn"]),
    lang("ko", "Korean", &["kor"]),
    lang("no", "Norwegian", &["nor", "nob", "nno", "nb", "nn"]),
    lang("pl", "Polish", &["pol"]),
    lang("pt", "Portuguese", &["por"]),
    lang("ro", "Romanian", &["ron", "rum"]),
    lang("ru", "Russian", &["rus"]),
    lang("es", "Spanish", &["spa"]),
    lang("sv", "Swedish", &["swe"]),
    lang("tr", "Turkish", &["tur"]),
    lang("uk", "Ukrainian", &["ukr"]),
];

const fn lang(
    code: &'static str,
    name: &'static str,
    track_codes: &'static [&'static str],
) -> Language {
    Language {
        code,
        name,
        track_codes,
    }
}

pub fn find(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code == code)
}

/// The mpv `alang`/`slang` list for a configured language code, empty when unset.
pub fn mpv_language_list(code: &str) -> String {
    match find(code) {
        Some(language) => language
            .track_codes
            .iter()
            .copied()
            .chain(std::iter::once(language.code))
            .collect::<Vec<_>>()
            .join(","),
        None => String::new(),
    }
}

/// A track language tag reduced to one comparable form: the configured code
/// for known languages (`"ger"`, `"deu"` and `"de-AT"` all become `"de"`),
/// otherwise the lowercased tag without its region.
pub fn canonical(track_language: &str) -> String {
    let base = track_language
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|l| l.code == base || l.track_codes.contains(&base.as_str()))
        .map_or(base, |l| l.code.to_string())
}

/// Whether a track's language tag (`"ger"`, `"de"`, `"en-US"`, ...) is the configured language.
pub fn track_matches(track_language: &str, code: &str) -> bool {
    find(code).is_some_and(|language| canonical(track_language) == language.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpv_list_contains_all_codes() {
        assert_eq!(mpv_language_list("de"), "deu,ger,de");
        assert_eq!(mpv_language_list(""), "");
    }

    #[test]
    fn matches_bibliographic_terminology_and_region_codes() {
        assert!(track_matches("ger", "de"));
        assert!(track_matches("deu", "de"));
        assert!(track_matches("en-US", "en"));
        assert!(track_matches("ENG", "en"));
        assert!(!track_matches("eng", "de"));
        assert!(!track_matches("eng", ""));
    }

    #[test]
    fn canonical_unifies_known_codes_and_keeps_unknown_ones() {
        assert_eq!(canonical("ger"), canonical("deu"));
        assert_eq!(canonical("eng"), canonical("en-US"));
        assert_eq!(canonical("tlh"), "tlh");
    }
}
