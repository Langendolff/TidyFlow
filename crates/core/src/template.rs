use crate::facts::Facts;
use chrono::{DateTime, Local};

// ponytail: replace encadeado; um valor que contenha "{...}" pode ser substituído de novo.
// Trocar por um parser de um passo se isso virar problema real.
pub fn render(template: &str, f: &Facts) -> String {
    let t: DateTime<Local> = f.modified.into();
    template
        .replace("{year}", &t.format("%Y").to_string())
        .replace("{month}", &t.format("%m").to_string())
        .replace("{day}", &t.format("%d").to_string())
        .replace("{ext}", &f.ext)
        .replace("{name}", &f.stem)
        .replace(
            "{origin_host}",
            &f.origin_host().unwrap_or_else(|| "unknown".into()),
        )
        .replace("{site}", &f.site().unwrap_or_else(|| "unknown".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_variables() {
        // sample: 2026-06-15 12:00 UTC → mesmo dia em qualquer fuso
        let mut f = Facts::sample("Fatura.PDF");
        assert_eq!(
            render("/d/{year}/{month}-{day}/{name}.{ext}/{origin_host}", &f),
            "/d/2026/06-15/Fatura.pdf/unknown"
        );
        f.origin_url = Some("https://nubank.com.br/x".into());
        assert_eq!(render("{origin_host}", &f), "nubank.com.br");
        assert_eq!(render("{site}", &f), "nubank.com.br");
        f.referrer_url = Some("https://app.nubank.com.br/faturas".into());
        assert_eq!(
            render("{site}/{origin_host}", &f),
            "app.nubank.com.br/nubank.com.br"
        );
        assert_eq!(render("sem variáveis", &f), "sem variáveis");
    }
}
