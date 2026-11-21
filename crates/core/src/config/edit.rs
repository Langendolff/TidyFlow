//! Edição do `config.toml` pela interface, preservando comentários e formatação.
//!
//! Toda operação é validada com [`super::parse`] antes de valer: se o resultado for
//! inválido, o documento volta ao que era e nada é gravado.

use super::{ConfigError, parse};
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

/// Regra como o usuário escreveu (`"100MB"`, `"~/Downloads"`), para o formulário.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleDraft {
    pub name: String,
    pub enabled: bool,
    pub watch: Vec<String>,
    pub when: Vec<Condition>,
    pub action: DraftAction,
}

/// `ext` usa uma lista separada por vírgulas: `"pdf, xml"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum DraftAction {
    Move(String),
    Copy(String),
    Rename(String),
    Trash,
}

#[derive(Debug)]
pub enum EditError {
    Invalid(ConfigError),
    /// O arquivo mudou no disco desde que foi lido.
    Conflict,
    Io(io::Error),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EditError::Invalid(e) => e.fmt(f),
            EditError::Conflict => f.write_str("o config.toml mudou no disco desde que foi aberto"),
            EditError::Io(e) => write!(f, "erro ao gravar o config: {e}"),
        }
    }
}

impl std::error::Error for EditError {}

impl From<io::Error> for EditError {
    fn from(e: io::Error) -> Self {
        EditError::Io(e)
    }
}

fn invalid(message: impl Into<String>) -> EditError {
    EditError::Invalid(ConfigError {
        line: None,
        message: message.into(),
    })
}

pub struct Editor {
    path: PathBuf,
    doc: DocumentMut,
    /// mtime lido; `None` se o arquivo não existia.
    mtime: Option<SystemTime>,
}

impl Editor {
    /// Arquivo ausente = documento vazio (será criado no primeiro `write`).
    pub fn open(path: &Path) -> Result<Editor, EditError> {
        let (src, mtime) = match fs::read_to_string(path) {
            Ok(s) => (s, Some(fs::metadata(path)?.modified()?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (String::new(), None),
            Err(e) => return Err(e.into()),
        };
        Ok(Editor {
            path: path.to_path_buf(),
            doc: Editor::parse_doc(&src)?,
            mtime,
        })
    }

    /// Documento em memória, sem arquivo (validação de rascunhos).
    pub fn in_memory(src: &str) -> Result<Editor, EditError> {
        Ok(Editor {
            path: PathBuf::new(),
            doc: Editor::parse_doc(src)?,
            mtime: None,
        })
    }

    fn parse_doc(src: &str) -> Result<DocumentMut, EditError> {
        src.parse::<DocumentMut>().map_err(|e| {
            EditError::Invalid(ConfigError {
                line: e.span().map(|s| super::line_of(src, s.start)),
                message: e.message().to_string(),
            })
        })
    }

    /// mtime do arquivo quando foi lido (`None`: não existia).
    pub fn mtime(&self) -> Option<SystemTime> {
        self.mtime
    }

    pub fn drafts(&self) -> Result<Vec<RuleDraft>, EditError> {
        match self.rules() {
            Some(aot) => aot.iter().map(draft_of).collect(),
            None => Ok(Vec::new()),
        }
    }

    /// `index: None` cria no fim. Devolve o índice da regra salva.
    pub fn save_rule(
        &mut self,
        index: Option<usize>,
        draft: &RuleDraft,
    ) -> Result<usize, EditError> {
        let table = table_of(draft)?;
        self.change(|doc| {
            let aot = rules_mut(doc);
            match index {
                Some(i) if i < aot.len() => {
                    let old = aot.get(i).expect("índice conferido");
                    let mut table = table;
                    *table.decor_mut() = old.decor().clone();
                    table.set_position(old.position());
                    aot.replace(i, table);
                    Ok(i)
                }
                Some(i) => Err(invalid(format!("regra {} não existe", i + 1))),
                None => {
                    aot.push(table);
                    Ok(aot.len() - 1)
                }
            }
        })
    }

    pub fn delete_rule(&mut self, index: usize) -> Result<(), EditError> {
        self.change(|doc| {
            let aot = rules_mut(doc);
            if index >= aot.len() {
                return Err(invalid(format!("regra {} não existe", index + 1)));
            }
            aot.remove(index);
            Ok(())
        })
    }

    pub fn move_rule(&mut self, from: usize, to: usize) -> Result<(), EditError> {
        self.change(|doc| {
            let aot = rules_mut(doc);
            if from >= aot.len() || to >= aot.len() {
                return Err(invalid("posição de regra inválida"));
            }
            let mut tables: Vec<Table> = aot.iter().cloned().collect();
            // As posições no documento ficam onde estavam; só as regras trocam de lugar.
            let mut slots: Vec<Option<isize>> = tables.iter().map(Table::position).collect();
            slots.sort();
            let t = tables.remove(from);
            tables.insert(to, t);
            aot.clear();
            for (mut t, pos) in tables.into_iter().zip(slots) {
                t.set_position(pos);
                clear_child_positions(&mut t);
                aot.push(t);
            }
            Ok(())
        })
    }

    pub fn set_enabled(&mut self, index: usize, enabled: bool) -> Result<(), EditError> {
        self.change(|doc| {
            let t = rules_mut(doc)
                .get_mut(index)
                .ok_or_else(|| invalid(format!("regra {} não existe", index + 1)))?;
            if enabled {
                t.remove("enabled");
            } else {
                t.insert("enabled", toml_edit::value(false));
            }
            Ok(())
        })
    }

    /// Grava de forma atômica. Falha com `Conflict` se o arquivo mudou desde o `open`.
    pub fn write(&mut self) -> Result<(), EditError> {
        let current = fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if current != self.mtime {
            return Err(EditError::Conflict);
        }
        // Grava no destino real: um config.toml que é symlink (dotfiles) continua symlink.
        let target = fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        let dir = target.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        let name = target.file_name().unwrap_or_default().to_string_lossy();
        let tmp = dir.join(format!(".{name}.tidyflow-tmp"));
        let result = (|| {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(self.doc.to_string().as_bytes())?;
            f.sync_all()?;
            if let Ok(meta) = fs::metadata(&target) {
                fs::set_permissions(&tmp, meta.permissions())?;
            }
            fs::rename(&tmp, &target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
        self.mtime = Some(fs::metadata(&self.path)?.modified()?);
        Ok(())
    }

    fn rules(&self) -> Option<&ArrayOfTables> {
        self.doc.get("rule").and_then(Item::as_array_of_tables)
    }

    /// Aplica `f` numa cópia; só fica com o resultado se ele passar no `parse`.
    fn change<T>(
        &mut self,
        f: impl FnOnce(&mut DocumentMut) -> Result<T, EditError>,
    ) -> Result<T, EditError> {
        let mut doc = self.doc.clone();
        let out = f(&mut doc)?;
        parse(&doc.to_string()).map_err(EditError::Invalid)?;
        self.doc = doc;
        Ok(out)
    }
}

impl fmt::Display for Editor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.doc.fmt(f)
    }
}

fn rules_mut(doc: &mut DocumentMut) -> &mut ArrayOfTables {
    let item = doc
        .entry("rule")
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
    if !item.is_array_of_tables() {
        *item = Item::ArrayOfTables(ArrayOfTables::new());
    }
    item.as_array_of_tables_mut().expect("acabou de ser criado")
}

/// Subtabelas (`[rule.when]`) passam a herdar a posição da regra.
fn clear_child_positions(t: &mut Table) {
    for (_, item) in t.iter_mut() {
        if let Some(child) = item.as_table_mut() {
            child.set_position(None);
            clear_child_positions(child);
        }
    }
}

const CONDITION_KEYS: [&str; 9] = [
    "ext",
    "name",
    "name_regex",
    "mime",
    "origin",
    "site",
    "size_gt",
    "size_lt",
    "older_than",
];

fn table_of(d: &RuleDraft) -> Result<Table, EditError> {
    let name = d.name.trim();
    if name.is_empty() {
        return Err(invalid("a regra precisa de um nome"));
    }
    let mut when = InlineTable::new();
    for c in &d.when {
        let key = c.key.as_str();
        if !CONDITION_KEYS.contains(&key) {
            return Err(invalid(format!("condição desconhecida: {key}")));
        }
        if when.contains_key(key) {
            return Err(invalid(format!("condição repetida: {key}")));
        }
        let value: Value = if key == "ext" {
            let exts: Array = c
                .value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            if exts.is_empty() {
                return Err(invalid("a condição ext está vazia"));
            }
            Value::Array(exts)
        } else {
            let v = c.value.trim();
            if v.is_empty() {
                return Err(invalid(format!("a condição {key} está vazia")));
            }
            v.into()
        };
        when.insert(key, value);
    }
    let mut action = InlineTable::new();
    match &d.action {
        DraftAction::Move(v) => action.insert("move", v.trim().into()),
        DraftAction::Copy(v) => action.insert("copy", v.trim().into()),
        DraftAction::Rename(v) => action.insert("rename", v.trim().into()),
        DraftAction::Trash => action.insert("trash", true.into()),
    };
    let watch: Array = d
        .watch
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .collect();

    let mut t = Table::new();
    t.insert("name", toml_edit::value(name));
    if !d.enabled {
        t.insert("enabled", toml_edit::value(false));
    }
    t.insert("watch", toml_edit::value(watch));
    t.insert("when", toml_edit::value(when));
    t.insert("action", toml_edit::value(action));
    // Alinha os `=` como no config de exemplo.
    let width = t.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let keys: Vec<String> = t.iter().map(|(k, _)| k.to_string()).collect();
    for k in keys {
        if let Some(mut key) = t.key_mut(&k) {
            key.leaf_decor_mut()
                .set_suffix(" ".repeat(width - k.len() + 1));
        }
    }
    Ok(t)
}

fn draft_of(t: &Table) -> Result<RuleDraft, EditError> {
    let str_of = |v: &Value| match v.as_str() {
        Some(s) => s.to_string(),
        None => v.to_string().trim().to_string(),
    };
    let name = t
        .get("name")
        .and_then(Item::as_str)
        .ok_or_else(|| invalid("regra sem nome"))?
        .to_string();
    let enabled = t.get("enabled").and_then(Item::as_bool).unwrap_or(true);
    let watch = t
        .get("watch")
        .and_then(Item::as_array)
        .map(|a| a.iter().map(str_of).collect())
        .unwrap_or_default();
    let mut when = Vec::new();
    if let Some(w) = t.get("when").and_then(Item::as_table_like) {
        for (key, item) in w.iter() {
            let value = match item.as_value() {
                Some(Value::Array(a)) => a.iter().map(str_of).collect::<Vec<_>>().join(", "),
                Some(v) => str_of(v),
                None => String::new(),
            };
            when.push(Condition {
                key: key.to_string(),
                value,
            });
        }
    }
    let action = t
        .get("action")
        .and_then(Item::as_table_like)
        .and_then(|a| {
            let (key, item) = a.iter().next()?;
            let v = item.as_value().map(str_of).unwrap_or_default();
            Some(match key {
                "move" => DraftAction::Move(v),
                "copy" => DraftAction::Copy(v),
                "rename" => DraftAction::Rename(v),
                _ => DraftAction::Trash,
            })
        })
        .ok_or_else(|| invalid(format!("regra {name:?} sem ação")))?;
    Ok(RuleDraft {
        name,
        enabled,
        watch,
        when,
        action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../../config.example.toml");

    fn ed() -> Editor {
        Editor::in_memory(EXAMPLE).unwrap()
    }

    fn names(e: &Editor) -> Vec<String> {
        parse(&e.to_string())
            .unwrap()
            .rules
            .into_iter()
            .map(|r| r.name)
            .collect()
    }

    fn draft(name: &str) -> RuleDraft {
        RuleDraft {
            name: name.into(),
            enabled: true,
            watch: vec!["~/Downloads".into()],
            when: vec![Condition {
                key: "ext".into(),
                value: "pdf, xml".into(),
            }],
            action: DraftAction::Move("~/Docs/{year}/".into()),
        }
    }

    /// Bloco de texto de uma regra no exemplo, do `[[rule]]` até a linha vazia.
    fn block<'a>(src: &'a str, name: &str) -> &'a str {
        let start = src.find(&format!("name   = \"{name}\"")).unwrap();
        let start = src[..start].rfind("[[rule]]").unwrap();
        let end = src[start..].find("\n\n").map_or(src.len(), |e| start + e);
        &src[start..end]
    }

    #[test]
    fn drafts_roundtrip() {
        let d = ed().drafts().unwrap();
        assert_eq!(d.len(), 5);
        assert_eq!(d[3].name, "Instaladores grandes antigos");
        let when: Vec<_> = d[3]
            .when
            .iter()
            .map(|c| (c.key.as_str(), c.value.as_str()))
            .collect();
        assert_eq!(
            when,
            vec![
                ("ext", "iso, AppImage, deb"),
                ("size_gt", "100MB"),
                ("older_than", "14d")
            ]
        );
        assert_eq!(d[3].action, DraftAction::Trash);
        assert_eq!(d[0].watch, vec!["~/Downloads".to_string()]);
        assert_eq!(
            d[0].action,
            DraftAction::Move("~/Documentos/Finanças/{year}/".into())
        );
    }

    #[test]
    fn save_preserves_comments_and_others() {
        let mut e = ed();
        let mut d = e.drafts().unwrap()[1].clone();
        d.when[0].value = "pdf".into();
        e.save_rule(Some(1), &d).unwrap();
        let out = e.to_string();
        assert!(out.contains("# Primeira regra que casar vence"));
        assert!(out.starts_with("[settings]\nage_scan_interval = \"1h\""));
        for other in [
            "Faturas Nubank",
            "Imagens por site",
            "Instaladores grandes antigos",
            "Limpeza geral",
        ] {
            assert_eq!(block(&out, other), block(EXAMPLE, other), "{other}");
        }
        assert!(out.contains("when   = { ext = [\"pdf\"], name_regex = \"(?i)nota|nf-?e\" }"));
        assert_eq!(e.drafts().unwrap()[1], d);
    }

    #[test]
    fn site_condition_roundtrips() {
        let mut e = ed();
        let mut d = draft("Do GitHub");
        d.when = vec![Condition {
            key: "site".into(),
            value: "github.com".into(),
        }];
        let i = e.save_rule(None, &d).unwrap();
        assert!(e.to_string().contains("when   = { site = \"github.com\" }"));
        assert_eq!(e.drafts().unwrap()[i], d);
    }

    #[test]
    fn save_new_appends() {
        let mut e = ed();
        assert_eq!(e.save_rule(None, &draft("Nova")).unwrap(), 5);
        assert_eq!(names(&e).last().unwrap(), "Nova");
        assert!(e.to_string().ends_with(
            "[[rule]]\nname   = \"Nova\"\nwatch  = [\"~/Downloads\"]\nwhen   = { ext = [\"pdf\", \"xml\"] }\naction = { move = \"~/Docs/{year}/\" }\n"
        ));
    }

    #[test]
    fn invalid_draft_is_rejected_and_doc_unchanged() {
        let mut e = ed();
        let before = e.to_string();
        let mut d = draft("Ruim");
        d.when[0] = Condition {
            key: "name_regex".into(),
            value: "(".into(),
        };
        let Err(EditError::Invalid(err)) = e.save_rule(None, &d) else {
            panic!("esperava Invalid");
        };
        assert!(err.message.contains("regex inválida"), "{err}");
        // nome repetido
        assert!(matches!(
            e.save_rule(None, &draft("Limpeza geral")),
            Err(EditError::Invalid(_))
        ));
        // condição vazia
        let mut empty = draft("Vazia");
        empty.when[0].value = " , ".into();
        assert!(matches!(
            e.save_rule(None, &empty),
            Err(EditError::Invalid(_))
        ));
        assert_eq!(e.to_string(), before);
    }

    #[test]
    fn move_rule_reorders() {
        let mut e = ed();
        e.move_rule(4, 0).unwrap();
        assert_eq!(
            names(&e),
            vec![
                "Limpeza geral",
                "Faturas Nubank",
                "Notas fiscais",
                "Imagens por site",
                "Instaladores grandes antigos"
            ]
        );
        assert!(e.to_string().starts_with("[settings]"));
        e.move_rule(0, 4).unwrap();
        assert_eq!(names(&e)[4], "Limpeza geral");
        assert!(e.move_rule(0, 9).is_err());
    }

    #[test]
    fn delete_and_enable() {
        let mut e = ed();
        e.delete_rule(0).unwrap();
        assert_eq!(names(&e)[0], "Notas fiscais");
        e.set_enabled(0, false).unwrap();
        assert!(e.to_string().contains("enabled = false"));
        assert!(!parse(&e.to_string()).unwrap().rules[0].enabled);
        assert!(!e.drafts().unwrap()[0].enabled);
        e.set_enabled(0, true).unwrap();
        assert!(!e.to_string().contains("enabled"));
        assert!(e.delete_rule(10).is_err());
    }

    #[test]
    fn reads_and_moves_subtable_style() {
        let src = "[[rule]]\nname = \"a\"\nwatch = [\"/tmp\"]\n\n[rule.when]\next = [\"pdf\"]\n\n[rule.action]\ntrash = true\n\n[[rule]]\nname = \"b\"\nwatch = [\"/tmp\"]\nwhen = { ext = [\"png\"] }\naction = { trash = true }\n";
        let mut e = Editor::in_memory(src).unwrap();
        let d = e.drafts().unwrap();
        assert_eq!(d[0].when[0].value, "pdf");
        assert_eq!(d[0].action, DraftAction::Trash);
        e.move_rule(1, 0).unwrap();
        let cfg = parse(&e.to_string()).unwrap();
        assert_eq!(cfg.rules[0].name, "b");
        assert_eq!(cfg.rules[1].when.ext, Some(vec!["pdf".to_string()]));
    }

    #[test]
    fn empty_file_gets_first_rule() {
        let mut e = Editor::in_memory("").unwrap();
        assert!(e.drafts().unwrap().is_empty());
        e.save_rule(None, &draft("Primeira")).unwrap();
        assert_eq!(names(&e), vec!["Primeira"]);
    }

    #[test]
    fn write_is_atomic_and_detects_conflict() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/config.toml");
        let mut e = Editor::open(&p).unwrap(); // ainda não existe
        e.save_rule(None, &draft("A")).unwrap();
        e.write().unwrap();
        e.save_rule(None, &draft("B")).unwrap();
        e.write().unwrap(); // mtime atualizado: sem conflito
        assert_eq!(
            parse(&fs::read_to_string(&p).unwrap()).unwrap().rules.len(),
            2
        );

        let mut stale = Editor::open(&p).unwrap();
        let outside = "# editado à mão\n";
        fs::write(&p, outside).unwrap();
        let f = fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        stale.delete_rule(0).unwrap();
        assert!(matches!(stale.write(), Err(EditError::Conflict)));
        assert_eq!(fs::read_to_string(&p).unwrap(), outside);
        assert!(!d.path().join("sub/.config.toml.tidyflow-tmp").exists());
    }

    #[test]
    fn write_keeps_symlink() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("dotfiles/config.toml");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, EXAMPLE).unwrap();
        let link = d.path().join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut e = Editor::open(&link).unwrap();
        e.delete_rule(0).unwrap();
        e.write().unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            !fs::read_to_string(&real)
                .unwrap()
                .contains("Faturas Nubank")
        );
    }

    #[test]
    fn draft_json_shape() {
        let json = serde_json::to_value(draft("x")).unwrap();
        assert_eq!(json["action"]["kind"], "move");
        assert_eq!(json["action"]["value"], "~/Docs/{year}/");
        let t: DraftAction = serde_json::from_str(r#"{"kind":"trash"}"#).unwrap();
        assert_eq!(t, DraftAction::Trash);
    }
}
