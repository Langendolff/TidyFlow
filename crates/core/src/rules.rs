use crate::{
    config::{Conditions, Rule},
    facts::Facts,
    units::{format_duration, format_size},
};
use std::time::SystemTime;

/// Resultado de uma condição: `expected` é o valor da regra, legível.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Check {
    pub key: &'static str,
    pub expected: String,
    pub ok: bool,
}

/// Uma entrada por condição presente, na ordem da tabela de condições.
pub fn explain(c: &Conditions, f: &Facts, now: SystemTime) -> Vec<Check> {
    let mut v = Vec::new();
    let mut push = |key, expected: String, ok| v.push(Check { key, expected, ok });
    if let Some(exts) = &c.ext {
        push("ext", exts.join(", "), exts.contains(&f.ext));
    }
    if let Some(g) = &c.name {
        push("name", g.glob().glob().to_string(), g.is_match(&f.name));
    }
    if let Some(r) = &c.name_regex {
        push("name_regex", r.as_str().to_string(), r.is_match(&f.name));
    }
    if let Some(g) = &c.mime {
        push("mime", g.glob().glob().to_string(), g.is_match(&f.mime));
    }
    if let Some(g) = &c.origin {
        let ok = f.origin_host().is_some_and(|h| g.is_match(&h));
        push("origin", g.glob().glob().to_string(), ok);
    }
    if let Some(g) = &c.site {
        let ok = f.site().is_some_and(|h| g.is_match(&h));
        push("site", g.glob().glob().to_string(), ok);
    }
    if let Some(n) = c.size_gt {
        push("size_gt", format!("> {}", format_size(n)), f.size > n);
    }
    if let Some(n) = c.size_lt {
        push("size_lt", format!("< {}", format_size(n)), f.size < n);
    }
    if let Some(d) = c.older_than {
        push(
            "older_than",
            format!("> {}", format_duration(d)),
            f.age(now) > d,
        );
    }
    v
}

pub fn matches(c: &Conditions, f: &Facts, now: SystemTime) -> bool {
    explain(c, f, now).iter().all(|c| c.ok)
}

/// Primeira regra ativa que casa, entre as que monitoram a pasta do arquivo.
pub fn first_match<'a>(rules: &'a [Rule], f: &Facts, now: SystemTime) -> Option<&'a Rule> {
    let dir = f.path.parent()?;
    pick(rules, f, now, |r| r.watch.iter().any(|w| w == dir))
}

/// Primeira regra ativa que casa, qualquer que seja o `watch` (pedido explícito do usuário).
pub fn first_match_anywhere<'a>(rules: &'a [Rule], f: &Facts, now: SystemTime) -> Option<&'a Rule> {
    pick(rules, f, now, |_| true)
}

fn pick<'a>(
    rules: &'a [Rule],
    f: &Facts,
    now: SystemTime,
    applies: impl Fn(&Rule) -> bool,
) -> Option<&'a Rule> {
    rules
        .iter()
        .find(|r| r.enabled && applies(r) && matches(&r.when, f, now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, Config};
    use std::time::Duration;

    fn cfg(rules: &[(&str, &str)]) -> Config {
        let tmp = std::env::temp_dir().display().to_string();
        let src: String = rules
            .iter()
            .map(|(name, when)| {
                format!("[[rule]]\nname = \"{name}\"\nwatch = ['{tmp}']\nwhen = {when}\naction = {{ trash = true }}\n\n")
            })
            .collect();
        config::parse(&src).unwrap()
    }

    fn hit(when: &str, f: &Facts) -> bool {
        matches(&cfg(&[("r", when)]).rules[0].when, f, f.modified)
    }

    #[test]
    fn each_condition() {
        let mut f = Facts::sample("Nota_Fiscal.PDF");
        f.mime = "application/pdf".into();
        f.size = 5_000;
        f.origin_url = Some("https://www.receita.gov.br/x".into());
        f.referrer_url = Some("https://portal.gov.br/servicos".into());

        let table = [
            (r#"{ ext = ["pdf"] }"#, true),
            (r#"{ ext = ["png", "jpg"] }"#, false),
            (r#"{ name = "nota_*" }"#, true), // glob ignora maiúsculas
            (r#"{ name = "*.zip" }"#, false),
            (r#"{ name_regex = "(?i)fiscal" }"#, true),
            (r#"{ name_regex = "^fiscal" }"#, false),
            (r#"{ mime = "application/*" }"#, true),
            (r#"{ mime = "image/*" }"#, false),
            (r#"{ origin = "*.gov.br" }"#, true),
            (r#"{ origin = "*.nubank.com.br" }"#, false),
            (r#"{ site = "portal.gov.br" }"#, true), // site vem do referrer
            (r#"{ site = "www.receita.gov.br" }"#, false),
            (r#"{ size_gt = "1KB" }"#, true),
            (r#"{ size_gt = "5KB" }"#, false), // estritamente maior
            (r#"{ size_lt = "1MB" }"#, true),
            (r#"{ size_lt = "1KB" }"#, false),
            (r#"{ ext = ["pdf"], size_gt = "10KB" }"#, false), // E lógico
        ];
        for (when, expected) in table {
            assert_eq!(hit(when, &f), expected, "{when}");
        }
    }

    #[test]
    fn explain_agrees_with_matches() {
        let mut f = Facts::sample("Nota_Fiscal.PDF");
        f.mime = "application/pdf".into();
        f.size = 5_000;
        for (when, expected) in [
            (r#"{ ext = ["pdf"], size_gt = "1KB" }"#, true),
            (r#"{ ext = ["pdf"], size_gt = "10KB" }"#, false),
            (r#"{ origin = "*" }"#, false),
        ] {
            let c = cfg(&[("r", when)]);
            let checks = explain(&c.rules[0].when, &f, f.modified);
            assert_eq!(checks.iter().all(|c| c.ok), expected, "{when}");
        }
    }

    #[test]
    fn explain_lists_present_conditions_in_order() {
        let c = cfg(&[(
            "r",
            r#"{ older_than = "14d", size_gt = "100MB", name_regex = "^a", ext = ["iso", "deb"] }"#,
        )]);
        let f = Facts::sample("a.iso");
        let checks = explain(&c.rules[0].when, &f, f.modified);
        let got: Vec<_> = checks
            .iter()
            .map(|c| (c.key, c.expected.as_str(), c.ok))
            .collect();
        assert_eq!(
            got,
            vec![
                ("ext", "iso, deb", true),
                ("name_regex", "^a", true),
                ("size_gt", "> 100 MB", false),
                ("older_than", "> 14 dias", false),
            ]
        );
    }

    #[test]
    fn explain_puts_site_after_origin() {
        let c = cfg(&[("r", r#"{ site = "github.com", origin = "*", mime = "*" }"#)]);
        let mut f = Facts::sample("a.zip");
        f.origin_url = Some("https://objects.githubusercontent.com/x".into());
        f.referrer_url = Some("https://github.com/a/b".into());
        let keys: Vec<_> = explain(&c.rules[0].when, &f, f.modified)
            .iter()
            .map(|c| (c.key, c.ok))
            .collect();
        assert_eq!(keys, vec![("mime", true), ("origin", true), ("site", true)]);
    }

    #[test]
    fn first_match_anywhere_ignores_watch_but_not_paused() {
        let mut c = cfg(&[
            ("pdf", r#"{ ext = ["pdf"] }"#),
            ("tudo", r#"{ name = "*" }"#),
        ]);
        let mut f = Facts::sample("a.pdf");
        f.path = "/outra/pasta/a.pdf".into();
        assert_eq!(
            first_match_anywhere(&c.rules, &f, f.modified).unwrap().name,
            "pdf"
        );
        c.rules[0].enabled = false;
        assert_eq!(
            first_match_anywhere(&c.rules, &f, f.modified).unwrap().name,
            "tudo"
        );
    }

    #[test]
    fn origin_condition_needs_origin() {
        let f = Facts::sample("a.pdf");
        assert!(!hit(r#"{ origin = "*" }"#, &f));
    }

    #[test]
    fn older_than() {
        let c = cfg(&[("r", r#"{ older_than = "1d" }"#)]);
        let f = Facts::sample("a.pdf");
        let day = Duration::from_secs(86_400);
        assert!(!matches(&c.rules[0].when, &f, f.modified + day));
        assert!(matches(
            &c.rules[0].when,
            &f,
            f.modified + day + Duration::from_secs(1)
        ));
    }

    #[test]
    fn first_match_wins_and_respects_watch() {
        let c = cfg(&[
            ("pdf", r#"{ ext = ["pdf"] }"#),
            ("tudo", r#"{ name = "*" }"#),
        ]);
        let f = Facts::sample("a.pdf");
        assert_eq!(first_match(&c.rules, &f, f.modified).unwrap().name, "pdf");
        let g = Facts::sample("a.txt");
        assert_eq!(first_match(&c.rules, &g, g.modified).unwrap().name, "tudo");

        let mut paused = cfg(&[
            ("pdf", r#"{ ext = ["pdf"] }"#),
            ("tudo", r#"{ name = "*" }"#),
        ]);
        paused.rules[0].enabled = false;
        assert_eq!(
            first_match(&paused.rules, &f, f.modified).unwrap().name,
            "tudo"
        );

        let mut outside = Facts::sample("a.pdf");
        outside.path = "/outra/pasta/a.pdf".into();
        assert!(first_match(&c.rules, &outside, outside.modified).is_none());
    }
}
