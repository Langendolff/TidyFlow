use std::time::Duration;

/// "100MB" → 100_000_000. KB/MB/GB na base 1000; KiB/MiB/GiB na base 1024.
pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: f64 = num
        .parse()
        .map_err(|_| format!("tamanho inválido: {s:?}"))?;
    let mult: u64 = match unit.trim() {
        "" | "B" => 1,
        "KB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        u => return Err(format!("unidade de tamanho desconhecida: {u:?}")),
    };
    Ok((n * mult as f64) as u64)
}

/// "30d" → 30 dias. Unidades: s, m, h, d, w (obrigatória).
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: u64 = num
        .parse()
        .map_err(|_| format!("duração inválida: {s:?}"))?;
    let secs = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        u => {
            return Err(format!(
                "unidade de duração desconhecida: {u:?} (use s, m, h, d ou w)"
            ));
        }
    };
    let total = n
        .checked_mul(secs)
        .ok_or_else(|| format!("duração grande demais: {s:?}"))?;
    Ok(Duration::from_secs(total))
}

/// 100_000_000 → "100 MB" (base 1000, uma casa decimal com vírgula).
pub fn format_size(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        return format!("{n} B");
    }
    let s = format!("{:.1}", v);
    let s = s.strip_suffix(".0").unwrap_or(&s).replace('.', ",");
    format!("{s} {}", UNITS[i])
}

/// 14 dias → "14 dias"; usa a maior unidade que divide a duração exatamente.
pub fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0 => "0 s".into(),
        _ if s.is_multiple_of(86_400) => match s / 86_400 {
            1 => "1 dia".into(),
            n => format!("{n} dias"),
        },
        _ if s.is_multiple_of(3_600) => format!("{} h", s / 3_600),
        _ if s.is_multiple_of(60) => format!("{} min", s / 60),
        _ => format!("{s} s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("100").unwrap(), 100);
        assert_eq!(parse_size("100B").unwrap(), 100);
        assert_eq!(parse_size("2KB").unwrap(), 2_000);
        assert_eq!(parse_size("1.5MB").unwrap(), 1_500_000);
        assert_eq!(parse_size("1GB").unwrap(), 1_000_000_000);
        assert_eq!(parse_size("1MiB").unwrap(), 1_048_576);
        assert_eq!(parse_size(" 3 KiB ").unwrap(), 3_072);
        assert!(parse_size("10XB").is_err());
        assert!(parse_size("MB").is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("12h").unwrap(), Duration::from_secs(43_200));
        assert_eq!(
            parse_duration("30d").unwrap(),
            Duration::from_secs(2_592_000)
        );
        assert_eq!(
            parse_duration("2w").unwrap(),
            Duration::from_secs(1_209_600)
        );
        assert!(parse_duration("30").is_err());
        assert!(parse_duration("d").is_err());
        assert!(parse_duration("3y").is_err());
        assert!(parse_duration("99999999999999999d").is_err());
    }

    #[test]
    fn formats() {
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(100_000_000), "100 MB");
        assert_eq!(format_size(3_200_000_000), "3,2 GB");
        assert_eq!(format_size(1_500), "1,5 KB");
        assert_eq!(format_duration(Duration::from_secs(14 * 86_400)), "14 dias");
        assert_eq!(format_duration(Duration::from_secs(86_400)), "1 dia");
        assert_eq!(format_duration(Duration::from_secs(12 * 3600)), "12 h");
        assert_eq!(format_duration(Duration::from_secs(90)), "90 s");
        assert_eq!(format_duration(Duration::from_secs(1800)), "30 min");
    }
}
