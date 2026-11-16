use crate::{
    config::{Conditions, Rule},
    facts::Facts,
};
use std::time::SystemTime;

pub fn matches(c: &Conditions, f: &Facts, now: SystemTime) -> bool {
    c.ext.as_ref().is_none_or(|exts| exts.contains(&f.ext))
        && c.name.as_ref().is_none_or(|g| g.is_match(&f.name))
        && c.name_regex.as_ref().is_none_or(|r| r.is_match(&f.name))
        && c.mime.as_ref().is_none_or(|g| g.is_match(&f.mime))
        && c.origin
            .as_ref()
            .is_none_or(|g| f.origin_host().is_some_and(|h| g.is_match(&h)))
        && c.size_gt.is_none_or(|n| f.size > n)
        && c.size_lt.is_none_or(|n| f.size < n)
        && c.older_than.is_none_or(|d| f.age(now) > d)
}

pub fn first_match<'a>(rules: &'a [Rule], f: &Facts, now: SystemTime) -> Option<&'a Rule> {
    let dir = f.path.parent()?;
    rules
        .iter()
        .find(|r| r.enabled && r.watch.iter().any(|w| w == dir) && matches(&r.when, f, now))
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
                format!("[[rule]]\nname = \"{name}\"\nwatch = [\"{tmp}\"]\nwhen = {when}\naction = {{ trash = true }}\n\n")
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
