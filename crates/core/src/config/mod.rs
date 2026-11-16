pub mod edit;

use crate::units::{parse_duration, parse_size};
use globset::{GlobBuilder, GlobMatcher};
use regex::Regex;
use serde::Deserialize;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug)]
pub struct Config {
    pub age_scan_interval: Duration,
    pub notify: bool,
    pub rules: Vec<Rule>,
}

#[derive(Debug)]
pub struct Rule {
    pub name: String,
    /// `false` = pausada: a regra fica no config mas não é aplicada.
    pub enabled: bool,
    pub watch: Vec<PathBuf>,
    pub when: Conditions,
    pub action: Action,
}

#[derive(Debug)]
pub struct Conditions {
    pub ext: Option<Vec<String>>,
    pub name: Option<GlobMatcher>,
    pub name_regex: Option<Regex>,
    pub mime: Option<GlobMatcher>,
    pub origin: Option<GlobMatcher>,
    pub size_gt: Option<u64>,
    pub size_lt: Option<u64>,
    pub older_than: Option<Duration>,
}

impl Conditions {
    fn is_empty(&self) -> bool {
        self.ext.is_none()
            && self.name.is_none()
            && self.name_regex.is_none()
            && self.mime.is_none()
            && self.origin.is_none()
            && self.size_gt.is_none()
            && self.size_lt.is_none()
            && self.older_than.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Move(String),
    Copy(String),
    Rename(String),
    Trash,
}

#[derive(Debug)]
pub struct ConfigError {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "linha {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn default_path() -> PathBuf {
    crate::xdg_dir("XDG_CONFIG_HOME", ".config").join("tidyflow/config.toml")
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let src = fs::read_to_string(path).map_err(|e| ConfigError {
        line: None,
        message: format!("não foi possível ler {}: {e}", path.display()),
    })?;
    parse(&src)
}

impl Config {
    /// Pastas `watch` que não existem (únicas, ordenadas).
    pub fn missing_watch_dirs(&self) -> Vec<&Path> {
        let mut v: Vec<&Path> = self
            .rules
            .iter()
            .flat_map(|r| &r.watch)
            .map(PathBuf::as_path)
            .filter(|p| !p.is_dir())
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

pub fn parse(src: &str) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(src).map_err(|e| ConfigError {
        line: e.span().map(|s| line_of(src, s.start)),
        message: e.message().to_string(),
    })?;
    let age_scan_interval = raw
        .settings
        .age_scan_interval
        .as_deref()
        .map(parse_duration)
        .transpose()
        .map_err(|message| ConfigError {
            line: None,
            message,
        })?
        .unwrap_or(Duration::from_secs(3600));
    if age_scan_interval.is_zero() {
        return Err(ConfigError {
            line: None,
            message: "age_scan_interval precisa ser maior que zero".into(),
        });
    }
    let mut names = std::collections::HashSet::new();
    let rules = raw
        .rules
        .into_iter()
        .map(|spanned| {
            let line = line_of(src, spanned.span().start);
            let at = |message| ConfigError {
                line: Some(line),
                message,
            };
            let rule = compile_rule(spanned.into_inner()).map_err(at)?;
            if !names.insert(rule.name.clone()) {
                return Err(at(format!(
                    "regra {:?}: nome repetido (cada regra precisa de um nome único)",
                    rule.name
                )));
            }
            Ok(rule)
        })
        .collect::<Result<_, _>>()?;
    Ok(Config {
        age_scan_interval,
        notify: raw.settings.notify.unwrap_or(true),
        rules,
    })
}

fn line_of(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].matches('\n').count() + 1
}

fn expand_tilde(s: &str) -> String {
    match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            format!("{}{rest}", crate::home())
        }
        _ => s.to_string(),
    }
}

fn glob(pattern: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| format!("glob inválido {pattern:?}: {e}"))
}

fn compile_rule(r: RawRule) -> Result<Rule, String> {
    let name = r.name;
    let err = |m: String| format!("regra {name:?}: {m}");

    if r.watch.is_empty() {
        return Err(err("`watch` está vazio".into()));
    }
    let watch = r
        .watch
        .iter()
        .map(|w| PathBuf::from(expand_tilde(w)))
        .collect::<Vec<_>>();

    let w = r.when;
    let when = Conditions {
        ext: w.ext.map(|v| {
            v.iter()
                .map(|e| e.trim_start_matches('.').to_lowercase())
                .collect()
        }),
        name: w.name.as_deref().map(glob).transpose().map_err(err)?,
        name_regex: w
            .name_regex
            .as_deref()
            .map(|r| Regex::new(r).map_err(|e| format!("regex inválida: {e}")))
            .transpose()
            .map_err(err)?,
        mime: w.mime.as_deref().map(glob).transpose().map_err(err)?,
        origin: w.origin.as_deref().map(glob).transpose().map_err(err)?,
        size_gt: w
            .size_gt
            .as_deref()
            .map(parse_size)
            .transpose()
            .map_err(err)?,
        size_lt: w
            .size_lt
            .as_deref()
            .map(parse_size)
            .transpose()
            .map_err(err)?,
        older_than: w
            .older_than
            .as_deref()
            .map(parse_duration)
            .transpose()
            .map_err(err)?,
    };
    if when.is_empty() {
        return Err(err("`when` precisa de pelo menos uma condição".into()));
    }

    let a = r.action;
    let actions: Vec<Action> = [
        a.r#move.map(|d| Action::Move(expand_tilde(&d))),
        a.copy.map(|d| Action::Copy(expand_tilde(&d))),
        a.rename.map(Action::Rename),
        a.trash.filter(|t| *t).map(|_| Action::Trash),
    ]
    .into_iter()
    .flatten()
    .collect();
    let Ok([action]) = <[Action; 1]>::try_from(actions) else {
        return Err(err(
            "`action` precisa de exatamente uma ação: move, copy, rename ou trash".into(),
        ));
    };

    Ok(Rule {
        name,
        enabled: r.enabled,
        watch,
        when,
        action,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    settings: RawSettings,
    #[serde(default, rename = "rule")]
    rules: Vec<toml::Spanned<RawRule>>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawSettings {
    age_scan_interval: Option<String>,
    notify: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    name: String,
    #[serde(default = "yes")]
    enabled: bool,
    watch: Vec<String>,
    #[serde(default)]
    when: RawWhen,
    #[serde(default)]
    action: RawAction,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawWhen {
    ext: Option<Vec<String>>,
    name: Option<String>,
    name_regex: Option<String>,
    mime: Option<String>,
    origin: Option<String>,
    size_gt: Option<String>,
    size_lt: Option<String>,
    older_than: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawAction {
    r#move: Option<String>,
    copy: Option<String>,
    rename: Option<String>,
    trash: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> String {
        std::env::temp_dir().display().to_string()
    }

    fn rule(when: &str, action: &str) -> String {
        format!(
            "[[rule]]\nname = \"r\"\nwatch = [\"{}\"]\nwhen = {when}\naction = {action}\n",
            tmp()
        )
    }

    #[test]
    fn parses_full_rule() {
        let src = format!(
            "[settings]\nage_scan_interval = \"30m\"\nnotify = false\n\n{}",
            rule(
                r#"{ ext = [".PDF", "xml"], name = "*nota*", name_regex = "(?i)nf", mime = "application/*", origin = "*.gov.br", size_gt = "1KB", size_lt = "10MB", older_than = "2d" }"#,
                r#"{ move = "~/Docs/{year}/" }"#,
            )
        );
        let cfg = parse(&src).unwrap();
        assert_eq!(cfg.age_scan_interval, Duration::from_secs(1800));
        assert!(!cfg.notify);
        let r = &cfg.rules[0];
        assert_eq!(r.name, "r");
        assert_eq!(r.watch, vec![PathBuf::from(tmp())]);
        assert_eq!(r.when.ext, Some(vec!["pdf".to_string(), "xml".to_string()]));
        assert_eq!(r.when.size_gt, Some(1_000));
        assert_eq!(r.when.older_than, Some(Duration::from_secs(172_800)));
        assert_eq!(
            r.action,
            Action::Move(format!("{}/Docs/{{year}}/", crate::home()))
        );
    }

    #[test]
    fn defaults() {
        let cfg = parse(&rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }")).unwrap();
        assert_eq!(cfg.age_scan_interval, Duration::from_secs(3600));
        assert!(cfg.notify);
        assert_eq!(cfg.rules[0].action, Action::Trash);
    }

    #[test]
    fn syntax_error_has_line() {
        let err = parse("[settings]\nnotify = \n").unwrap_err();
        assert_eq!(err.line, Some(2));
    }

    #[test]
    fn unknown_key_has_line() {
        let src = format!(
            "[[rule]]\nname = \"r\"\nwatch = [\"{}\"]\naction = {{ trash = true }}\n\n[rule.when]\nfoo = 1\n",
            tmp()
        );
        let err = parse(&src).unwrap_err();
        assert!(err.message.contains("foo"), "{err}");
        assert_eq!(err.line, Some(7));
    }

    #[test]
    fn semantic_errors_name_the_rule() {
        let cases = [
            rule("{}", "{ trash = true }"),
            rule(r#"{ ext = ["pdf"] }"#, "{}"),
            rule(r#"{ ext = ["pdf"] }"#, r#"{ trash = true, move = "/x" }"#),
            rule(r#"{ name_regex = "(" }"#, "{ trash = true }"),
            rule(r#"{ size_gt = "10XB" }"#, "{ trash = true }"),
        ];
        for src in cases {
            let err = parse(&src).unwrap_err();
            assert!(err.message.contains("regra \"r\""), "{err}");
            assert_eq!(err.line, Some(1), "{err}");
        }
    }

    #[test]
    fn zero_interval_is_rejected() {
        let src = format!(
            "[settings]\nage_scan_interval = \"0s\"\n\n{}",
            rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }")
        );
        let err = parse(&src).unwrap_err();
        assert!(err.message.contains("maior que zero"), "{err}");
    }

    #[test]
    fn missing_watch_dirs_are_listed_not_errors() {
        let src = "[[rule]]\nname = \"r\"\nwatch = [\"/nao/existe/b\", \"/nao/existe/a\", \"/nao/existe/b\", \"/tmp\"]\nwhen = { ext = [\"pdf\"] }\naction = { trash = true }\n";
        let cfg = parse(src).unwrap();
        assert_eq!(
            cfg.missing_watch_dirs(),
            vec![Path::new("/nao/existe/a"), Path::new("/nao/existe/b")]
        );
    }

    #[test]
    fn enabled_defaults_to_true_and_can_be_false() {
        let on = parse(&rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }")).unwrap();
        assert!(on.rules[0].enabled);
        let src = rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }")
            .replace("name = \"r\"\n", "name = \"r\"\nenabled = false\n");
        assert!(!parse(&src).unwrap().rules[0].enabled);
    }

    #[test]
    fn duplicate_rule_names_are_rejected() {
        let r = rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }");
        let err = parse(&format!("{r}\n{r}")).unwrap_err();
        assert!(err.message.contains("nome repetido"), "{err}");
        assert_eq!(err.line, Some(7)); // linha do segundo [[rule]]
    }

    #[test]
    fn display_includes_line() {
        let e = ConfigError {
            line: Some(3),
            message: "x".into(),
        };
        assert_eq!(e.to_string(), "linha 3: x");
    }
}
