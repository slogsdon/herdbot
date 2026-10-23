//! `herdbot config list|get|set|unset`: the supported settings registry, its read views and
//! the validated, format-preserving writes.
//!
//! TOML stays the source of truth. This module reads `herdbot.toml` and a bot's `bot.toml`,
//! validates every value before it writes, and edits the source with `toml_edit` so comments,
//! key order and unrelated sections survive. `routing.toml` is deliberately out of scope
//! (settings design, non-goals).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use herdbot_core::home::{self, Result};
use serde::Serialize;
use serde_json::{Map as JsonMap, Value as Json, json};
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

/// The `--json` contract version. Add fields, don't rename them.
pub const CONTRACT_VERSION: u32 = 1;

const IMMEDIATE: &str = "immediate";
const NEXT_BOT_START: &str = "next-bot-start";

pub const USAGE: &str = "usage:
  herdbot config list [--bot BOT] [--json]       supported settings, values and apply timing
  herdbot config get <key> [--bot BOT] [--json]  one setting
  herdbot config set <key> <value> [--bot BOT] [--json]  validate and write one setting
  herdbot config unset <key> [--bot BOT] [--json]        remove one setting (its default applies)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Bot,
}

impl Scope {
    fn name(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Bot => "bot",
        }
    }
}

/// A setting's fallback when it is absent from TOML. Kept as data so tests can assert the
/// registry rather than a second copy of the defaults.
#[derive(Clone, Copy)]
enum Fallback {
    Text(&'static str),
    Number(i64),
    Flag(bool),
    Unset,
    EmptyMap,
    BotFolder,
}

/// How a string setting's value is validated before it is written.
#[derive(Clone, Copy, PartialEq)]
enum Text {
    /// Not a string setting.
    None,
    /// Any single line, including an empty one.
    Line,
    /// A non-empty path; spaces are allowed.
    Path,
    /// One non-empty word; whitespace is rejected.
    Word,
}

#[derive(Clone, Copy)]
struct Definition {
    key: &'static str,
    /// `enum`, `string`, `integer`, `boolean` or `map`; serialised as `type`.
    kind: &'static str,
    description: &'static str,
    choices: &'static [&'static str],
    fallback: Fallback,
    apply: &'static str,
    /// Smallest accepted integer, when the type is `integer`.
    min: Option<i64>,
    /// This bot setting falls back to the global `approvals.mode` when absent.
    inherits_global_approvals: bool,
    /// Validation for a string setting's written value.
    text: Text,
}

const GLOBAL_SETTINGS: &[Definition] = &[
    Definition {
        key: "approvals.mode",
        kind: "enum",
        description: "Whether agents ask before each tool action (ask) or use the harness's automatic review (auto). A bot's approvals setting overrides this.",
        choices: &["ask", "auto"],
        fallback: Fallback::Text("ask"),
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: false,
        text: Text::None,
    },
    Definition {
        key: "codex.sandbox",
        kind: "enum",
        description: "Codex sandbox mode. \"none\" runs codex with danger-full-access so it can write outside its workspace; \"default\" keeps the workspace-write sandbox.",
        choices: &["default", "none"],
        fallback: Fallback::Text("default"),
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: false,
        text: Text::None,
    },
    Definition {
        key: "memory.max_compactors",
        kind: "integer",
        description: "Maximum memory compactor runs across all bots at once. 0 stops new compactor runs.",
        choices: &[],
        fallback: Fallback::Number(2),
        apply: IMMEDIATE,
        min: Some(0),
        inherits_global_approvals: false,
        text: Text::None,
    },
    Definition {
        key: "paths",
        kind: "map",
        description: "Path prefixes rewritten in a bot's imported instructions when it starts. The longest matching key wins and a leading ~ expands to the home directory.",
        choices: &[],
        fallback: Fallback::EmptyMap,
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: false,
        text: Text::None,
    },
    Definition {
        key: "cua.host",
        kind: "string",
        description: "SSH host that runs cua-driver for computer_use agents. Unset means this Mac.",
        choices: &[],
        fallback: Fallback::Unset,
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: false,
        text: Text::Word,
    },
    Definition {
        key: "cua.command",
        kind: "string",
        description: "cua-driver command on the computer_use host or its PATH. Unset means cua-driver.",
        choices: &[],
        fallback: Fallback::Unset,
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: false,
        text: Text::Word,
    },
];

const BOT_SETTINGS: &[Definition] = &[
    Definition {
        key: "title",
        kind: "string",
        description: "Display title shown in bot listings. The bot's slug remains its identifier.",
        choices: &[],
        fallback: Fallback::Text(""),
        apply: IMMEDIATE,
        min: None,
        inherits_global_approvals: false,
        text: Text::Line,
    },
    Definition {
        key: "cwd",
        kind: "string",
        description: "Working directory for the bot's workspace and agents; a leading ~ expands. Unset means the bot's own folder.",
        choices: &[],
        fallback: Fallback::BotFolder,
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: false,
        text: Text::Path,
    },
    Definition {
        key: "class",
        kind: "string",
        description: "Routing class for the bot's main agent; the class resolves through routing.toml.",
        choices: &[],
        fallback: Fallback::Text("default"),
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: false,
        text: Text::Word,
    },
    Definition {
        key: "approvals",
        kind: "enum",
        description: "Approval mode for this bot's main agent and subagents. Unset inherits the global approvals.mode.",
        choices: &["ask", "auto"],
        fallback: Fallback::Text("ask"),
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: true,
        text: Text::None,
    },
    Definition {
        key: "memory",
        kind: "boolean",
        description: "Keep an OptChat memory log (the daemon starts logging on its next pass) and add a view of it to the bot's instructions at its next start.",
        choices: &[],
        fallback: Fallback::Flag(false),
        apply: NEXT_BOT_START,
        min: None,
        inherits_global_approvals: false,
        text: Text::None,
    },
    Definition {
        key: "memory_view_bytes",
        kind: "integer",
        description: "Byte budget for the memory view in a memory bot's instructions. The built-in default is 64000.",
        choices: &[],
        fallback: Fallback::Number(64_000),
        apply: NEXT_BOT_START,
        min: Some(0),
        inherits_global_approvals: false,
        text: Text::None,
    },
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Setting {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub description: &'static str,
    pub default: Json,
    pub value: Json,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<&'static str>>,
    pub apply: &'static str,
}

#[derive(Serialize)]
struct ListReport<'a> {
    version: u32,
    scope: &'static str,
    settings: &'a [Setting],
}

#[derive(Serialize)]
struct GetReport<'a> {
    version: u32,
    scope: &'static str,
    setting: &'a Setting,
}

fn definitions(scope: Scope) -> &'static [Definition] {
    match scope {
        Scope::Global => GLOBAL_SETTINGS,
        Scope::Bot => BOT_SETTINGS,
    }
}

fn definition(scope: Scope, key: &str) -> Option<&'static Definition> {
    definitions(scope).iter().find(|definition| definition.key == key)
}

/// `Some(bot)` selects the bot scope, `None` the global scope.
fn scope_of(bot: Option<&str>) -> Scope {
    if bot.is_some() { Scope::Bot } else { Scope::Global }
}

/// Bot ids are folder names: only an already-normalised `home::slug` may be used.
fn bot_slug(bot: &str) -> Result<&str> {
    if bot.is_empty() || home::slug(bot) != bot {
        return Err(format!(
            "invalid bot id {bot:?}: expected a lowercase slug (a-z, 0-9 and -), at most 20 characters"
        )
        .into());
    }
    Ok(bot)
}

fn settings_path(home_dir: &Path, scope: Scope, bot: Option<&str>) -> Result<PathBuf> {
    match scope {
        Scope::Global => Ok(home_dir.join("herdbot.toml")),
        Scope::Bot => {
            let slug = bot_slug(bot.ok_or("--bot is required for bot settings")?)?;
            Ok(home_dir.join(slug).join("bot.toml"))
        }
    }
}

fn parse_toml(path: &Path, text: &str) -> Result<toml::Table> {
    text.parse::<toml::Table>()
        .map_err(|error| format!("{} is not valid TOML: {error}", path.display()).into())
}

/// `herdbot.toml` is optional: its absence means every global setting is at its default.
fn read_optional(path: &Path) -> Result<toml::Table> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_toml(path, &text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(error) => Err(format!("cannot read {}: {error}", path.display()).into()),
    }
}

fn read_required(path: &Path) -> Result<toml::Table> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse_toml(path, &text)
}

fn load(home_dir: &Path, scope: Scope, bot: Option<&str>) -> Result<(PathBuf, toml::Table)> {
    let path = settings_path(home_dir, scope, bot)?;
    let table = match scope {
        Scope::Global => read_optional(&path)?,
        Scope::Bot => {
            let slug = bot.ok_or("--bot is required for bot settings")?;
            if !path.is_file() {
                return Err(format!(
                    "no bot {slug:?} in {} (missing {}); see `herdbot list`",
                    home_dir.display(),
                    path.display()
                )
                .into());
            }
            read_required(&path)?
        }
    };
    Ok((path, table))
}

/// Dotted lookup: `approvals.mode` finds `[approvals] mode`.
fn lookup<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let mut parts = key.split('.');
    let mut value = table.get(parts.next()?)?;
    for part in parts {
        value = value.get(part)?;
    }
    Some(value)
}

struct Reader<'a> {
    home: &'a Path,
    bot: Option<&'a str>,
    path: &'a Path,
    table: &'a toml::Table,
}

impl Reader<'_> {
    fn setting(&self, definition: &'static Definition) -> Result<Setting> {
        let raw = lookup(self.table, definition.key);
        let value = match definition.kind {
            "enum" => match raw {
                Some(value) => {
                    let text = self.string(definition, value)?;
                    if !definition.choices.contains(&text) {
                        return Err(self.invalid(definition, value, self.expected_choices(definition)));
                    }
                    Json::String(text.to_owned())
                }
                None if definition.inherits_global_approvals => {
                    Json::String(self.global_approvals()?)
                }
                None => self.fallback(definition),
            },
            "integer" => match raw {
                Some(value) => {
                    let number = value.as_integer().ok_or_else(|| {
                        self.invalid(definition, value, "an integer".into())
                    })?;
                    if let Some(min) = definition.min
                        && number < min
                    {
                        return Err(self.invalid(definition, value, format!("an integer >= {min}")));
                    }
                    Json::Number(number.into())
                }
                None => self.fallback(definition),
            },
            "boolean" => match raw {
                Some(value) => Json::Bool(value.as_bool().ok_or_else(|| {
                    self.invalid(definition, value, "true or false".into())
                })?),
                None => self.fallback(definition),
            },
            "string" => match raw {
                Some(value) => Json::String(self.string(definition, value)?.to_owned()),
                None => self.fallback(definition),
            },
            "map" => match raw {
                Some(value) => self.map(definition, value)?,
                None => self.fallback(definition),
            },
            other => unreachable!("setting {} has unknown type {other}", definition.key),
        };
        Ok(Setting {
            key: definition.key.to_owned(),
            kind: definition.kind,
            description: definition.description,
            default: self.fallback(definition),
            value,
            choices: (!definition.choices.is_empty()).then(|| definition.choices.to_vec()),
            apply: definition.apply,
        })
    }

    fn fallback(&self, definition: &Definition) -> Json {
        match definition.fallback {
            Fallback::Text(text) => Json::String(text.to_owned()),
            Fallback::Number(number) => json!(number),
            Fallback::Flag(flag) => Json::Bool(flag),
            Fallback::Unset => Json::Null,
            Fallback::EmptyMap => Json::Object(JsonMap::new()),
            Fallback::BotFolder => Json::String(
                self.home
                    .join(self.bot.unwrap_or_default())
                    .to_string_lossy()
                    .into_owned(),
            ),
        }
    }

    fn string<'a>(&self, definition: &Definition, value: &'a toml::Value) -> Result<&'a str> {
        value
            .as_str()
            .ok_or_else(|| self.invalid(definition, value, "a string".into()))
    }

    fn map(&self, definition: &Definition, value: &toml::Value) -> Result<Json> {
        let table = value.as_table().ok_or_else(|| {
            self.invalid(definition, value, "a table of string = string".into())
        })?;
        let mut out = JsonMap::new();
        for (key, value) in table {
            let text = value.as_str().ok_or_else(|| {
                self.invalid(
                    definition,
                    value,
                    format!("a string for the {} entry {key:?}", definition.key),
                )
            })?;
            out.insert(key.clone(), Json::String(text.to_owned()));
        }
        Ok(Json::Object(out))
    }

    /// The effective global approval mode, used when a bot does not override it.
    fn global_approvals(&self) -> Result<String> {
        let path = self.home.join("herdbot.toml");
        let table = read_optional(&path)?;
        let definition =
            definition(Scope::Global, "approvals.mode").expect("approvals.mode is registered");
        let reader = Reader {
            home: self.home,
            bot: None,
            path: &path,
            table: &table,
        };
        let setting = reader.setting(definition)?;
        Ok(setting
            .value
            .as_str()
            .unwrap_or("ask")
            .to_owned())
    }

    fn invalid(
        &self,
        definition: &Definition,
        value: &toml::Value,
        expected: String,
    ) -> Box<dyn std::error::Error + Send + Sync> {
        format!(
            "invalid {} = {} in {}: expected {}",
            definition.key,
            value,
            self.path.display(),
            expected
        )
        .into()
    }

    fn expected_choices(&self, definition: &Definition) -> String {
        expected_choices(definition)
    }
}

fn expected_choices(definition: &Definition) -> String {
    format!(
        "one of {}",
        definition
            .choices
            .iter()
            .map(|choice| format!("{choice:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn unknown_setting(scope: Scope, key: &str) -> Box<dyn std::error::Error + Send + Sync> {
    if key == "routing" || key.starts_with("routing.") {
        return format!(
            "unknown setting {key:?}: routing.toml is not managed by `herdbot config`; edit it directly"
        )
        .into();
    }
    let supported = definitions(scope)
        .iter()
        .map(|definition| definition.key)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "unknown {} setting {key:?}; supported: {supported} (see `herdbot config list{})",
        scope.name(),
        if scope == Scope::Bot { " --bot BOT" } else { "" }
    )
    .into()
}

/// Every supported setting for the scope, with metadata defaults for absent values.
pub fn list(home_dir: &Path, bot: Option<&str>) -> Result<Vec<Setting>> {
    let scope = scope_of(bot);
    let (path, table) = load(home_dir, scope, bot)?;
    let reader = Reader {
        home: home_dir,
        bot,
        path: &path,
        table: &table,
    };
    definitions(scope)
        .iter()
        .map(|definition| reader.setting(definition))
        .collect()
}

/// One supported setting. Unknown keys and invalid stored values are errors.
pub fn get(home_dir: &Path, bot: Option<&str>, key: &str) -> Result<Setting> {
    let scope = scope_of(bot);
    let definition = definition(scope, key).ok_or_else(|| unknown_setting(scope, key))?;
    let (path, table) = load(home_dir, scope, bot)?;
    Reader {
        home: home_dir,
        bot,
        path: &path,
        table: &table,
    }
    .setting(definition)
}

/// A `set` value that already passed validation.
enum NewValue {
    Scalar(Value),
    /// One entry for a `map` setting, written as `FROM = "TO"`.
    MapEntry { from: String, to: String },
}

fn invalid_value(definition: &Definition, raw: &str, expected: &str) -> Box<dyn std::error::Error + Send + Sync> {
    format!(
        "invalid value {raw:?} for {}: expected {expected}",
        definition.key
    )
    .into()
}

fn validate_text(definition: &Definition, raw: &str) -> Result<String> {
    if raw.chars().any(char::is_control) {
        return Err(invalid_value(
            definition,
            raw,
            "a single line without control characters",
        ));
    }
    match definition.text {
        Text::Line => Ok(raw.to_owned()),
        Text::Path if raw.trim().is_empty() => {
            Err(invalid_value(definition, raw, "a non-empty path"))
        }
        Text::Path => Ok(raw.to_owned()),
        Text::Word if raw.trim().is_empty() || raw.chars().any(char::is_whitespace) => Err(
            invalid_value(definition, raw, "one non-empty word without spaces"),
        ),
        Text::Word => Ok(raw.to_owned()),
        Text::None => unreachable!("{} is a string without a text rule", definition.key),
    }
}

/// Validate one `config set` value before any file is read or written.
fn validate(definition: &Definition, raw: &str) -> Result<NewValue> {
    match definition.kind {
        "enum" => {
            if !definition.choices.contains(&raw) {
                return Err(invalid_value(
                    definition,
                    raw,
                    &expected_choices(definition),
                ));
            }
            Ok(NewValue::Scalar(Value::from(raw)))
        }
        "integer" => {
            let expected = match definition.min {
                Some(min) => format!("an integer >= {min}"),
                None => "an integer".to_owned(),
            };
            match raw.parse::<i64>() {
                Ok(number) if definition.min.is_none_or(|min| number >= min) => {
                    Ok(NewValue::Scalar(Value::from(number)))
                }
                Ok(_) | Err(_) => Err(invalid_value(definition, raw, &expected)),
            }
        }
        "boolean" => match raw {
            "true" => Ok(NewValue::Scalar(Value::from(true))),
            "false" => Ok(NewValue::Scalar(Value::from(false))),
            _ => Err(invalid_value(definition, raw, "true or false")),
        },
        "string" => {
            let text = validate_text(definition, raw)?;
            Ok(NewValue::Scalar(Value::from(text)))
        }
        "map" => {
            let (from, to) = raw.split_once('=').ok_or_else(|| {
                invalid_value(
                    definition,
                    raw,
                    "FROM=TO, for example \"/workspace/vault=~/Documents/vault\"",
                )
            })?;
            let (from, to) = (from.trim(), to.trim());
            if from.is_empty() {
                return Err(invalid_value(definition, raw, "a non-empty FROM path before `=`"));
            }
            if to.is_empty() {
                return Err(invalid_value(definition, raw, "a non-empty TO path after `=`"));
            }
            if from.chars().chain(to.chars()).any(char::is_control) {
                return Err(invalid_value(
                    definition,
                    raw,
                    "a single line without control characters",
                ));
            }
            Ok(NewValue::MapEntry {
                from: from.to_owned(),
                to: to.to_owned(),
            })
        }
        other => unreachable!("{} has unknown type {other}", definition.key),
    }
}

/// The text before a document's first item: a blank line so a new section is separated from
/// the keys above it, unless the file is empty or already ends with one.
fn section_separator(source: &str) -> &'static str {
    if source.trim().is_empty() || source.ends_with("\n\n") {
        ""
    } else {
        "\n"
    }
}

fn parse_document(path: &Path, text: &str) -> Result<DocumentMut> {
    text.parse::<DocumentMut>()
        .map_err(|error| format!("{} is not valid TOML: {error}", path.display()).into())
}

/// The container that holds `key`'s leaf segment: the root table for a bare key, or the
/// mutated `[section]` for a dotted one. Missing sections are created as real tables.
fn container_for<'a>(
    table: &'a mut Table,
    parents: &[&str],
    definition: &Definition,
    path: &Path,
    separator: &str,
) -> Result<Container<'a>> {
    let Some((head, rest)) = parents.split_first() else {
        return Ok(Container::Table(table));
    };
    let item = table.entry(head).or_insert(Item::None);
    if let Item::None = item {
        let mut section = Table::new();
        if rest.is_empty() && !separator.is_empty() {
            section.decor_mut().set_prefix(separator);
        }
        *item = Item::Table(section);
    }
    match item {
        Item::Table(section) => container_for(section, rest, definition, path, ""),
        Item::Value(Value::InlineTable(section)) => {
            inline_container(section, rest, definition, path)
        }
        other => Err(not_a_table(definition, head, other.type_name(), path)),
    }
}

fn inline_container<'a>(
    table: &'a mut InlineTable,
    parents: &[&str],
    definition: &Definition,
    path: &Path,
) -> Result<Container<'a>> {
    let Some((head, rest)) = parents.split_first() else {
        return Ok(Container::Inline(table));
    };
    let value = table
        .entry((*head).to_owned())
        .or_insert(Value::InlineTable(InlineTable::new()));
    match value {
        Value::InlineTable(section) => inline_container(section, rest, definition, path),
        other => Err(not_a_table(definition, head, other.type_name(), path)),
    }
}

fn not_a_table(
    definition: &Definition,
    segment: &str,
    kind: &str,
    path: &Path,
) -> Box<dyn std::error::Error + Send + Sync> {
    format!(
        "cannot write {} in {}: {segment:?} is {kind}, not a table; edit the file directly",
        definition.key,
        path.display()
    )
    .into()
}

/// A table or inline table that a value is written into.
enum Container<'a> {
    Table(&'a mut Table),
    Inline(&'a mut InlineTable),
}

impl Container<'_> {
    fn set(&mut self, key: &str, value: Value) {
        match self {
            Container::Table(table) => set_table_value(table, key, Item::Value(value)),
            Container::Inline(table) => set_inline_value(table, key, value),
        }
    }

    fn set_map_entry(
        &mut self,
        definition: &Definition,
        key: &str,
        from: &str,
        to: &str,
        path: &Path,
        separator: &str,
    ) -> Result<()> {
        match self {
            Container::Table(table) => {
                let item = table.entry(key).or_insert(Item::None);
                if let Item::None = item {
                    let mut section = Table::new();
                    if !separator.is_empty() {
                        section.decor_mut().set_prefix(separator);
                    }
                    set_table_value(&mut section, from, Item::Value(Value::from(to)));
                    *item = Item::Table(section);
                    return Ok(());
                }
                match item {
                    Item::Table(section) => {
                        set_table_value(section, from, Item::Value(Value::from(to)));
                        Ok(())
                    }
                    Item::Value(Value::InlineTable(section)) => {
                        set_inline_value(section, from, Value::from(to));
                        Ok(())
                    }
                    other => Err(not_a_table(definition, key, other.type_name(), path)),
                }
            }
            Container::Inline(table) => {
                let value = table
                    .entry(key.to_owned())
                    .or_insert(Value::InlineTable(InlineTable::new()));
                match value {
                    Value::InlineTable(section) => {
                        set_inline_value(section, from, Value::from(to));
                        Ok(())
                    }
                    other => Err(format!(
                        "cannot write {} in {}: {key:?} is {}, not a table; edit the file directly",
                        definition.key,
                        path.display(),
                        other.type_name()
                    )
                    .into()),
                }
            }
        }
    }
}

/// Replace a value while keeping its decoration, which is where an inline `# comment` lives.
fn set_table_value(table: &mut Table, key: &str, item: Item) {
    let decor = table
        .get(key)
        .and_then(Item::as_value)
        .map(Value::decor)
        .cloned();
    let mut item = item;
    if let (Some(decor), Some(value)) = (decor, item.as_value_mut()) {
        *value.decor_mut() = decor;
    }
    table.insert(key, item);
}

fn set_inline_value(table: &mut InlineTable, key: &str, value: Value) {
    let decor = table.get(key).map(Value::decor).cloned();
    let mut value = value;
    if let Some(decor) = decor {
        *value.decor_mut() = decor;
    }
    table.insert(key, value);
}

fn edit(
    document: &mut DocumentMut,
    definition: &Definition,
    value: NewValue,
    path: &Path,
    separator: &str,
) -> Result<()> {
    let parts: Vec<&str> = definition.key.split('.').collect();
    let (leaf, parents) = parts.split_last().expect("a key has at least one segment");
    let mut container = container_for(document.as_table_mut(), parents, definition, path, separator)?;
    match value {
        NewValue::Scalar(value) => {
            container.set(leaf, value);
            Ok(())
        }
        NewValue::MapEntry { from, to } => container.set_map_entry(
            definition,
            leaf,
            &from,
            &to,
            path,
            if parents.is_empty() { separator } else { "" },
        ),
    }
}

/// Remove a dotted key. An emptied parent section header stays, and the whole edit is a no-op
/// (so the bytes stay untouched) when the key was absent. Returns whether it changed.
fn remove_key(document: &mut DocumentMut, key: &str) -> bool {
    let parts: Vec<&str> = key.split('.').collect();
    let (leaf, parents) = parts.split_last().expect("a key has at least one segment");
    remove_from_table(document.as_table_mut(), parents, leaf)
}

fn remove_from_table(table: &mut Table, parents: &[&str], leaf: &str) -> bool {
    let Some((head, rest)) = parents.split_first() else {
        return table.remove(leaf).is_some();
    };
    match table.get_mut(head) {
        Some(Item::Table(section)) => remove_from_table(section, rest, leaf),
        Some(Item::Value(Value::InlineTable(section))) => remove_from_inline(section, rest, leaf),
        _ => false,
    }
}

fn remove_from_inline(table: &mut InlineTable, parents: &[&str], leaf: &str) -> bool {
    let Some((head, rest)) = parents.split_first() else {
        return table.remove(leaf).is_some();
    };
    match table.get_mut(head) {
        Some(Value::InlineTable(section)) => remove_from_inline(section, rest, leaf),
        _ => false,
    }
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Write through a sibling temp file and rename it over `path`, so a reader (or a crash) never
/// sees a half-written file. The original's permissions are kept.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with(path, |file| std::io::Write::write_all(file, bytes))
}

fn write_atomic_with(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<()> {
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("herdbot.toml");
    let temp = directory.join(format!(
        ".{name}.{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        if let Ok(existing) = std::fs::metadata(path) {
            file.set_permissions(existing.permissions())?;
        }
        write(&mut file)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        // Best effort: make the rename itself durable.
        if let Ok(dir) = std::fs::File::open(directory) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(|error| format!("cannot write {}: {error}", path.display()).into())
}

fn read_source(path: &Path, scope: Scope) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && scope == Scope::Global => {
            Ok(None)
        }
        Err(error) => Err(format!("cannot read {}: {error}", path.display()).into()),
    }
}

/// Validate and write one setting, then return it as it now reads. Validation happens before
/// the file is opened, so a rejected value cannot change a byte.
pub fn set(home_dir: &Path, bot: Option<&str>, key: &str, raw: &str) -> Result<Setting> {
    let scope = scope_of(bot);
    let definition = definition(scope, key).ok_or_else(|| unknown_setting(scope, key))?;
    let value = validate(definition, raw)?;
    load(home_dir, scope, bot)?;
    let path = settings_path(home_dir, scope, bot)?;
    let source = read_source(&path, scope)?;
    let mut document = parse_document(&path, source.as_deref().unwrap_or_default())?;
    edit(
        &mut document,
        definition,
        value,
        &path,
        section_separator(source.as_deref().unwrap_or_default()),
    )?;
    let updated = serialize_checked(&path, source.as_deref(), document)?;
    let text = updated.as_deref().or(source.as_deref()).unwrap_or_default();
    let setting = reader_at(home_dir, bot, &path, text, definition)?;
    if let Some(updated) = updated {
        write_new(&path, &updated)?;
    }
    Ok(setting)
}

/// Remove one setting so its default (or, for a bot's `approvals`, the global mode) applies.
pub fn unset(home_dir: &Path, bot: Option<&str>, key: &str) -> Result<Setting> {
    let scope = scope_of(bot);
    let definition = definition(scope, key).ok_or_else(|| unknown_setting(scope, key))?;
    load(home_dir, scope, bot)?;
    let path = settings_path(home_dir, scope, bot)?;
    let source = read_source(&path, scope)?;
    let mut document = parse_document(&path, source.as_deref().unwrap_or_default())?;
    let updated = if remove_key(&mut document, definition.key) {
        serialize_checked(&path, source.as_deref(), document)?
    } else {
        None
    };
    let text = updated.as_deref().or(source.as_deref()).unwrap_or_default();
    let setting = reader_at(home_dir, bot, &path, text, definition)?;
    if let Some(updated) = updated {
        write_new(&path, &updated)?;
    }
    Ok(setting)
}

/// What a `get` would report for `text`, computed before the write so a document that reads
/// back badly (an invalid inherited global mode, say) never lands on disk.
fn reader_at(
    home_dir: &Path,
    bot: Option<&str>,
    path: &Path,
    text: &str,
    definition: &'static Definition,
) -> Result<Setting> {
    let table = parse_toml(path, text)?;
    Reader {
        home: home_dir,
        bot,
        path,
        table: &table,
    }
    .setting(definition)
}

/// Serialise the edited document, keep the file's final-newline style, and prove the result
/// still parses as a `toml::Table` the readers accept. `None` means the edit changed nothing.
fn serialize_checked(
    path: &Path,
    source: Option<&str>,
    document: DocumentMut,
) -> Result<Option<String>> {
    let mut updated = document.to_string();
    // `toml_edit` always terminates the document with a newline; a file that did not end with
    // one keeps its shape, which also makes an identical write a true no-op.
    if source.is_some_and(|source| !source.is_empty() && !source.ends_with('\n'))
        && updated.ends_with('\n')
    {
        updated.pop();
    }
    if source == Some(updated.as_str()) {
        return Ok(None);
    }
    updated.parse::<toml::Table>().map_err(|error| {
        format!(
            "refusing to write {}: the edit is not valid TOML: {error}",
            path.display()
        )
    })?;
    Ok(Some(updated))
}

fn write_new(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomic(path, text.as_bytes())
}

fn compact_json(value: &Json) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

fn apply_label(apply: &str) -> &str {
    match apply {
        IMMEDIATE => "immediately",
        NEXT_BOT_START => "on the bot's next start",
        "daemon-restart" => "after a daemon restart",
        other => other,
    }
}

fn choices_label(setting: &Setting) -> Option<String> {
    setting.choices.as_ref().map(|choices| {
        choices
            .iter()
            .map(|choice| format!("{choice:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

fn render_list(scope: Scope, path: &Path, settings: &[Setting]) -> String {
    let width = settings
        .iter()
        .map(|setting| setting.key.len())
        .max()
        .unwrap_or(0);
    let mut out = format!("{} settings ({})\n", scope.name(), path.display());
    for setting in settings {
        let mut line = format!(
            "{:<width$} = {}",
            setting.key,
            compact_json(&setting.value),
            width = width
        );
        if let Some(choices) = choices_label(setting) {
            line.push_str(&format!("  [one of {choices}]"));
        }
        line.push_str(&format!(
            "  default {}; applies {}",
            compact_json(&setting.default),
            apply_label(setting.apply)
        ));
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn render_setting(scope: Scope, path: &Path, setting: &Setting) -> String {
    let mut out = format!("{} = {}\n", setting.key, compact_json(&setting.value));
    out.push_str(&format!("  {}\n", setting.description));
    out.push_str(&format!("  type: {}", setting.kind));
    if let Some(choices) = choices_label(setting) {
        out.push_str(&format!("; one of {choices}"));
    }
    out.push('\n');
    out.push_str(&format!("  default: {}\n", compact_json(&setting.default)));
    out.push_str(&format!("  applies: {}\n", apply_label(setting.apply)));
    out.push_str(&format!(
        "  scope: {}; file: {}\n",
        scope.name(),
        path.display()
    ));
    out
}

fn render_change(verb: &str, path: &Path, setting: &Setting) -> String {
    let mut out = format!("{verb} {} in {}\n", setting.key, path.display());
    out.push_str(&format!(
        "  {} = {}\n",
        setting.key,
        compact_json(&setting.value)
    ));
    out.push_str(&format!("  default: {}\n", compact_json(&setting.default)));
    out.push_str(&format!("  applies: {}\n", apply_label(setting.apply)));
    out
}

#[derive(Debug, PartialEq)]
enum Command {
    List,
    Get(String),
    Set(String, String),
    Unset(String),
}

#[derive(Debug, PartialEq)]
struct Invocation {
    command: Command,
    bot: Option<String>,
    json: bool,
}

fn parse(args: Vec<String>) -> Result<Invocation> {
    let mut positional = Vec::new();
    let mut bot = None;
    let mut json = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--bot" => {
                let value = args.next().ok_or_else(|| format!("--bot needs a bot id\n{USAGE}"))?;
                if value.is_empty() {
                    return Err(format!("--bot needs a bot id\n{USAGE}").into());
                }
                bot = Some(value);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown flag {other:?}\n{USAGE}").into());
            }
            other => positional.push(other.to_owned()),
        }
    }
    let Some(command) = positional.first().cloned() else {
        return Err(USAGE.into());
    };
    let rest = &positional[1..];
    let command = match command.as_str() {
        "list" if rest.is_empty() => Command::List,
        "list" => return Err(format!("config list takes no arguments\n{USAGE}").into()),
        "get" => match rest {
            [key] => Command::Get(key.clone()),
            [] => return Err(format!("config get needs a key\n{USAGE}").into()),
            _ => return Err(format!("config get takes one key\n{USAGE}").into()),
        },
        "set" => match rest {
            [key, value] => Command::Set(key.clone(), value.clone()),
            [] | [_] => {
                return Err(format!("config set needs a key and a value\n{USAGE}").into());
            }
            _ => {
                return Err(format!(
                    "config set takes one key and one value (quote a value with spaces)\n{USAGE}"
                )
                .into());
            }
        },
        "unset" => match rest {
            [key] => Command::Unset(key.clone()),
            [] => return Err(format!("config unset needs a key\n{USAGE}").into()),
            _ => return Err(format!("config unset takes one key\n{USAGE}").into()),
        },
        other => {
            return Err(format!(
                "unknown config command {other:?}; supported: list, get, set, unset\n{USAGE}"
            )
            .into());
        }
    };
    Ok(Invocation { command, bot, json })
}

pub fn run(args: Vec<String>) -> Result<()> {
    let invocation = parse(args)?;
    let home_dir = home::root();
    let bot = invocation.bot.as_deref();
    let scope = scope_of(bot);
    let path = settings_path(&home_dir, scope, bot)?;
    match invocation.command {
        Command::List => {
            let settings = list(&home_dir, bot)?;
            if invocation.json {
                let report = ListReport {
                    version: CONTRACT_VERSION,
                    scope: scope.name(),
                    settings: &settings,
                };
                println!("{}", serde_json::to_string(&report)?);
            } else {
                print!("{}", render_list(scope, &path, &settings));
            }
        }
        Command::Get(key) => {
            let setting = get(&home_dir, bot, &key)?;
            if invocation.json {
                let report = GetReport {
                    version: CONTRACT_VERSION,
                    scope: scope.name(),
                    setting: &setting,
                };
                println!("{}", serde_json::to_string(&report)?);
            } else {
                print!("{}", render_setting(scope, &path, &setting));
            }
        }
        Command::Set(key, value) => {
            let setting = set(&home_dir, bot, &key, &value)?;
            if invocation.json {
                let report = GetReport {
                    version: CONTRACT_VERSION,
                    scope: scope.name(),
                    setting: &setting,
                };
                println!("{}", serde_json::to_string(&report)?);
            } else {
                print!("{}", render_change("set", &path, &setting));
            }
        }
        Command::Unset(key) => {
            let setting = unset(&home_dir, bot, &key)?;
            if invocation.json {
                let report = GetReport {
                    version: CONTRACT_VERSION,
                    scope: scope.name(),
                    setting: &setting,
                };
                println!("{}", serde_json::to_string(&report)?);
            } else {
                print!("{}", render_change("unset", &path, &setting));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TempHome {
        path: PathBuf,
    }

    impl TempHome {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "herdbot-config-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            TempHome { path }
        }

        fn write(&self, relative: &str, text: &str) -> PathBuf {
            let path = self.path.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn find<'a>(settings: &'a [Setting], key: &str) -> &'a Setting {
        settings
            .iter()
            .find(|setting| setting.key == key)
            .unwrap_or_else(|| panic!("{key} is not in the registry"))
    }

    fn error_of(result: Result<Setting>) -> String {
        result.unwrap_err().to_string()
    }

    fn assert_unchanged(path: &Path, before: &[u8]) {
        assert_eq!(std::fs::read(path).unwrap(), before, "{} was rewritten", path.display());
    }

    #[test]
    fn registry_inventory_matches_the_current_parsers() {
        assert_eq!(
            definitions(Scope::Global)
                .iter()
                .map(|definition| definition.key)
                .collect::<Vec<_>>(),
            [
                "approvals.mode",
                "codex.sandbox",
                "memory.max_compactors",
                "paths",
                "cua.host",
                "cua.command",
            ]
        );
        assert_eq!(
            definitions(Scope::Bot)
                .iter()
                .map(|definition| definition.key)
                .collect::<Vec<_>>(),
            [
                "title",
                "cwd",
                "class",
                "approvals",
                "memory",
                "memory_view_bytes",
            ]
        );
        for definition in GLOBAL_SETTINGS.iter().chain(BOT_SETTINGS) {
            assert!(!definition.description.is_empty(), "{} has no description", definition.key);
            assert!(
                [IMMEDIATE, NEXT_BOT_START, "daemon-restart"].contains(&definition.apply),
                "{} has unknown apply timing {:?}",
                definition.key,
                definition.apply
            );
            if definition.kind == "enum" {
                assert!(!definition.choices.is_empty(), "{} has no choices", definition.key);
            }
        }
    }

    #[test]
    fn list_returns_metadata_defaults_without_creating_the_file() {
        let home = TempHome::new("defaults");
        let settings = list(&home.path, None).unwrap();
        assert!(!home.path.join("herdbot.toml").exists());

        let mode = find(&settings, "approvals.mode");
        assert_eq!(mode.kind, "enum");
        assert_eq!(mode.default, json!("ask"));
        assert_eq!(mode.value, json!("ask"));
        assert_eq!(mode.choices, Some(vec!["ask", "auto"]));
        assert_eq!(mode.apply, NEXT_BOT_START);

        assert_eq!(find(&settings, "codex.sandbox").value, json!("default"));
        assert_eq!(find(&settings, "memory.max_compactors").value, json!(2));
        assert_eq!(find(&settings, "memory.max_compactors").apply, IMMEDIATE);
        assert_eq!(find(&settings, "paths").value, json!({}));
        assert_eq!(find(&settings, "cua.host").value, Json::Null);
        assert_eq!(find(&settings, "cua.command").value, Json::Null);
    }

    #[test]
    fn list_reads_global_values_and_preserves_comments_order_and_unrelated_keys() {
        let home = TempHome::new("global-values");
        let source = "\
# keep this comment
[approvals]
mode = \"auto\"   # inline comment

[paths]
\"/workspace/obsidian-vault\" = \"~/Documents/vault\"

[unrelated]
future = true
";
        let path = home.write("herdbot.toml", source);
        let settings = list(&home.path, None).unwrap();
        assert_eq!(find(&settings, "approvals.mode").value, json!("auto"));
        assert_eq!(
            find(&settings, "paths").value,
            json!({"/workspace/obsidian-vault": "~/Documents/vault"})
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    }

    #[test]
    fn bot_defaults_are_metadata_and_do_not_need_global_settings() {
        let home = TempHome::new("bot-defaults");
        let source = "name = \"alpha\"\n";
        let path = home.write("alpha/bot.toml", source);
        let settings = list(&home.path, Some("alpha")).unwrap();

        assert_eq!(find(&settings, "title").value, json!(""));
        assert_eq!(
            find(&settings, "cwd").value,
            json!(home.path.join("alpha").to_string_lossy())
        );
        assert_eq!(find(&settings, "cwd").default, find(&settings, "cwd").value);
        assert_eq!(find(&settings, "class").value, json!("default"));
        assert_eq!(find(&settings, "approvals").value, json!("ask"));
        assert_eq!(find(&settings, "approvals").default, json!("ask"));
        assert_eq!(find(&settings, "memory").value, json!(false));
        assert_eq!(find(&settings, "memory_view_bytes").value, json!(64_000));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        assert!(!home.path.join("herdbot.toml").exists());
    }

    #[test]
    fn bot_approvals_inherit_the_global_mode_unless_overridden() {
        let home = TempHome::new("approvals-inherit");
        home.write("herdbot.toml", "[approvals]\nmode = \"auto\"\n");
        home.write("alpha/bot.toml", "name = \"alpha\"\n");
        let inherited = list(&home.path, Some("alpha")).unwrap();
        assert_eq!(find(&inherited, "approvals").value, json!("auto"));
        assert_eq!(find(&inherited, "approvals").default, json!("ask"));

        home.write("alpha/bot.toml", "name = \"alpha\"\napprovals = \"ask\"\n");
        let overridden = list(&home.path, Some("alpha")).unwrap();
        assert_eq!(find(&overridden, "approvals").value, json!("ask"));
    }

    #[test]
    fn get_returns_one_setting_with_the_versioned_json_contract() {
        let home = TempHome::new("get");
        home.write("herdbot.toml", "[memory]\nmax_compactors = 4\n");
        let setting = get(&home.path, None, "memory.max_compactors").unwrap();
        assert_eq!(setting.value, json!(4));
        assert_eq!(setting.choices, None);

        let report = serde_json::to_value(GetReport {
            version: CONTRACT_VERSION,
            scope: "global",
            setting: &setting,
        })
        .unwrap();
        assert_eq!(report["version"], json!(1));
        assert_eq!(report["scope"], json!("global"));
        assert_eq!(report["setting"]["key"], json!("memory.max_compactors"));
        assert_eq!(report["setting"]["type"], json!("integer"));
        assert_eq!(report["setting"]["apply"], json!("immediate"));
        assert_eq!(report["setting"]["value"], json!(4));

        let bot = TempHome::new("get-bot");
        bot.write("alpha/bot.toml", "name = \"alpha\"\ntitle = \"Alpha\"\n");
        let title = get(&bot.path, Some("alpha"), "title").unwrap();
        assert_eq!(title.value, json!("Alpha"));
    }

    #[test]
    fn list_json_is_versioned_and_additive() {
        let home = TempHome::new("json");
        let settings = list(&home.path, None).unwrap();
        let report = serde_json::to_value(ListReport {
            version: CONTRACT_VERSION,
            scope: "global",
            settings: &settings,
        })
        .unwrap();
        assert_eq!(report["version"], json!(1));
        assert_eq!(report["scope"], json!("global"));
        let first = &report["settings"][0];
        assert_eq!(first["key"], json!("approvals.mode"));
        for field in ["key", "type", "description", "default", "value", "choices", "apply"] {
            assert!(first.get(field).is_some(), "missing {field}: {first}");
        }
        let integer = report["settings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|setting| setting["key"] == json!("memory.max_compactors"))
            .unwrap();
        assert!(integer.get("choices").is_none());
        assert!(report["settings"].as_array().unwrap().iter().all(|setting| {
            setting["key"].as_str().is_none_or(|key| !key.starts_with("routing"))
        }));
    }

    #[test]
    fn unknown_keys_name_the_supported_set_and_reject_routing() {
        let home = TempHome::new("unknown-key");
        let error = error_of(get(&home.path, None, "approvals"));
        assert!(error.contains("unknown global setting \"approvals\""), "{error}");
        assert!(error.contains("approvals.mode"), "{error}");
        assert!(error.contains("memory.max_compactors"), "{error}");

        home.write("alpha/bot.toml", "name = \"alpha\"\n");
        let error = error_of(get(&home.path, Some("alpha"), "approvals.mode"));
        assert!(error.contains("unknown bot setting \"approvals.mode\""), "{error}");
        assert!(error.contains("memory_view_bytes"), "{error}");

        let error = error_of(get(&home.path, None, "routing.default"));
        assert!(error.contains("routing.toml is not managed"), "{error}");
        assert!(!home.path.join("routing.toml").exists());
    }

    #[test]
    fn unknown_and_unsafe_bot_ids_are_rejected() {
        let home = TempHome::new("bot-ids");
        for bad in ["../alpha", "a/b", "Alpha", "", ".", "a".repeat(21).as_str()] {
            let error = list(&home.path, Some(bad)).unwrap_err().to_string();
            assert!(error.contains("invalid bot id"), "{bad:?}: {error}");
        }
        let error = list(&home.path, Some("ghost")).unwrap_err().to_string();
        assert!(error.contains("no bot \"ghost\""), "{error}");
        assert!(error.contains("ghost/bot.toml"), "{error}");
        assert!(!home.path.join("ghost").exists());
    }

    #[test]
    fn invalid_values_are_actionable_and_never_touch_the_file() {
        let cases = [
            (
                "global",
                "[approvals]\nmode = \"yolo\"\n",
                "approvals.mode",
                "one of \"ask\", \"auto\"",
            ),
            (
                "global",
                "[codex]\nsandbox = \"yolo\"\n",
                "codex.sandbox",
                "one of \"default\", \"none\"",
            ),
            (
                "global",
                "[memory]\nmax_compactors = \"two\"\n",
                "memory.max_compactors",
                "an integer",
            ),
            (
                "global",
                "[memory]\nmax_compactors = -1\n",
                "memory.max_compactors",
                "an integer >= 0",
            ),
            (
                "global",
                "[paths]\n\"/workspace\" = 3\n",
                "paths",
                "a string for the paths entry",
            ),
            (
                "global",
                "[cua]\nhost = true\n",
                "cua.host",
                "a string",
            ),
            ("bot", "name = \"alpha\"\napprovals = \"yolo\"\n", "approvals", "one of \"ask\", \"auto\""),
            ("bot", "name = \"alpha\"\nmemory = \"yes\"\n", "memory", "true or false"),
            ("bot", "name = \"alpha\"\nmemory_view_bytes = -5\n", "memory_view_bytes", "an integer >= 0"),
            ("bot", "name = \"alpha\"\ncwd = 42\n", "cwd", "a string"),
            ("bot", "name = \"alpha\"\ntitle = [\"x\"]\n", "title", "a string"),
            ("bot", "name = \"alpha\"\nclass = 1\n", "class", "a string"),
            ("bot", "name = \"alpha\"\n[approvals]\nmode = \"auto\"\n", "approvals", "a string"),
        ];
        for (index, (scope, source, key, expected)) in cases.into_iter().enumerate() {
            let home = TempHome::new(&format!("invalid-{index}"));
            let path = if scope == "bot" {
                home.write("alpha/bot.toml", source)
            } else {
                home.write("herdbot.toml", source)
            };
            let before = std::fs::read(&path).unwrap();
            let bot = (scope == "bot").then_some("alpha");
            let error = error_of(get(&home.path, bot, key));
            assert!(error.contains(key), "{scope} {key}: {error}");
            assert!(error.contains(expected), "{scope} {key}: {error}");
            assert!(error.contains(&path.to_string_lossy().to_string()), "{scope} {key}: {error}");
            assert_unchanged(&path, &before);
            let list_error = list(&home.path, bot).unwrap_err().to_string();
            assert!(list_error.contains(key), "list {scope} {key}: {list_error}");
            assert_unchanged(&path, &before);
        }
    }

    #[test]
    fn malformed_toml_names_the_file() {
        let home = TempHome::new("malformed");
        let path = home.write("herdbot.toml", "not = [toml\n");
        let error = error_of(get(&home.path, None, "approvals.mode"));
        assert!(error.contains("is not valid TOML"), "{error}");
        assert!(error.contains(&path.to_string_lossy().to_string()), "{error}");
        assert_unchanged(&path, b"not = [toml\n");
    }

    #[test]
    fn cli_parsing_is_order_independent_and_rejects_misuse() {
        assert_eq!(
            parse(vec!["list".into(), "--json".into(), "--bot".into(), "alpha".into()]).unwrap(),
            Invocation {
                command: Command::List,
                bot: Some("alpha".into()),
                json: true
            }
        );
        assert_eq!(
            parse(vec!["get".into(), "cwd".into(), "--bot".into(), "alpha".into()]).unwrap(),
            Invocation {
                command: Command::Get("cwd".into()),
                bot: Some("alpha".into()),
                json: false
            }
        );
        assert_eq!(
            parse(vec![
                "--json".into(),
                "set".into(),
                "title".into(),
                "Helper Bot".into(),
                "--bot".into(),
                "alpha".into()
            ])
            .unwrap(),
            Invocation {
                command: Command::Set("title".into(), "Helper Bot".into()),
                bot: Some("alpha".into()),
                json: true
            }
        );
        assert_eq!(
            parse(vec!["unset".into(), "memory".into(), "--bot".into(), "alpha".into()])
                .unwrap(),
            Invocation {
                command: Command::Unset("memory".into()),
                bot: Some("alpha".into()),
                json: false
            }
        );
        for (args, expected) in [
            (vec!["list", "--wat"], "unknown flag"),
            (vec!["list", "--bot"], "--bot needs a bot id"),
            (vec!["list", "extra"], "takes no arguments"),
            (vec!["get"], "needs a key"),
            (vec!["get", "a", "b"], "takes one key"),
            (vec!["set"], "needs a key and a value"),
            (vec!["set", "title"], "needs a key and a value"),
            (vec!["set", "title", "a", "b"], "quote a value with spaces"),
            (vec!["unset"], "needs a key"),
            (vec!["unset", "memory", "extra"], "takes one key"),
            (vec!["wat"], "supported: list, get, set, unset"),
        ] {
            let error = parse(args.into_iter().map(str::to_owned).collect())
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
        }
        assert!(parse(Vec::new()).unwrap_err().to_string().contains("usage"));
    }

    #[test]
    fn human_output_names_values_defaults_and_apply_timing() {
        let home = TempHome::new("human");
        let settings = list(&home.path, None).unwrap();
        let text = render_list(Scope::Global, &home.path.join("herdbot.toml"), &settings);
        assert!(text.contains("global settings"));
        assert!(text.contains("approvals.mode"));
        assert!(text.contains("= \"ask\""));
        assert!(text.contains("one of \"ask\", \"auto\""));
        assert!(text.contains("default \"ask\""));
        assert!(text.contains("applies on the bot's next start"));
        assert!(text.contains("applies immediately"));

        let setting = get(&home.path, None, "cua.host").unwrap();
        let text = render_setting(Scope::Global, &home.path.join("herdbot.toml"), &setting);
        assert!(text.contains("cua.host = null"));
        assert!(text.contains("SSH host"));
        assert!(text.contains("type: string"));
        assert!(text.contains("scope: global; file:"));
    }

    // ---- writes (Task 2) ----

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn inode(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).unwrap().ino()
    }

    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// Every entry in a directory, sorted: a leftover temp file fails the caller's expectation.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn assert_only(dir: &Path, expected: &[&str]) {
        let mut expected: Vec<String> = expected.iter().map(|name| (*name).to_owned()).collect();
        expected.sort();
        assert_eq!(entries(dir), expected, "unexpected files in {}", dir.display());
    }

    struct Sample {
        bot: bool,
        key: &'static str,
        value: &'static str,
        expected: Json,
        rejected: &'static str,
    }

    fn samples() -> Vec<Sample> {
        vec![
            Sample { bot: false, key: "approvals.mode", value: "auto", expected: json!("auto"), rejected: "yolo" },
            Sample { bot: false, key: "codex.sandbox", value: "none", expected: json!("none"), rejected: "danger" },
            Sample { bot: false, key: "memory.max_compactors", value: "7", expected: json!(7), rejected: "two" },
            Sample {
                bot: false,
                key: "paths",
                value: "/workspace/vault=~/Documents/vault",
                expected: json!({"/workspace/vault": "~/Documents/vault"}),
                rejected: "/workspace/vault",
            },
            Sample { bot: false, key: "cua.host", value: "shane@studio.local", expected: json!("shane@studio.local"), rejected: "two words" },
            Sample { bot: false, key: "cua.command", value: "~/.local/bin/cua-driver", expected: json!("~/.local/bin/cua-driver"), rejected: "" },
            Sample { bot: true, key: "title", value: "Helper Bot", expected: json!("Helper Bot"), rejected: "two\nlines" },
            Sample { bot: true, key: "cwd", value: "/Users/shane/My Projects/helper", expected: json!("/Users/shane/My Projects/helper"), rejected: "  " },
            Sample { bot: true, key: "class", value: "plan", expected: json!("plan"), rejected: "not a class" },
            Sample { bot: true, key: "approvals", value: "auto", expected: json!("auto"), rejected: "yolo" },
            Sample { bot: true, key: "memory", value: "true", expected: json!(true), rejected: "yes" },
            Sample { bot: true, key: "memory_view_bytes", value: "128000", expected: json!(128_000), rejected: "-1" },
        ]
    }

    fn write_home(bot: bool, name: &str) -> (TempHome, String) {
        let home = TempHome::new(name);
        let source = if bot {
            home.write("alpha/bot.toml", "name = \"alpha\"\n");
            "name = \"alpha\"\n"
        } else {
            ""
        };
        (home, source.to_owned())
    }

    #[test]
    fn every_registered_setting_accepts_and_rejects_a_sample_value() {
        let samples = samples();
        let mut covered: Vec<&str> = samples.iter().map(|sample| sample.key).collect();
        covered.sort_unstable();
        let mut registered: Vec<&str> = GLOBAL_SETTINGS
            .iter()
            .chain(BOT_SETTINGS)
            .map(|definition| definition.key)
            .collect();
        registered.sort_unstable();
        assert_eq!(covered, registered, "every setting needs a write sample");

        for sample in samples {
            let (home, before) = write_home(sample.bot, &format!("accept-{}", sample.key.replace('.', "-")));
            let bot = sample.bot.then_some("alpha");
            let setting = set(&home.path, bot, sample.key, sample.value).unwrap();
            assert_eq!(setting.value, sample.expected, "{} = {:?}", sample.key, sample.value);
            assert_eq!(get(&home.path, bot, sample.key).unwrap().value, sample.expected);
            assert_ne!(read(&home_path(&home, sample.bot)), before, "{} did not write", sample.key);

            let path = home_path(&home, sample.bot);
            let accepted = std::fs::read(&path).unwrap();
            let error = set(&home.path, bot, sample.key, sample.rejected)
                .unwrap_err()
                .to_string();
            assert!(error.contains(sample.key), "{}: {error}", sample.key);
            assert!(error.contains("expected"), "{}: {error}", sample.key);
            assert_eq!(std::fs::read(&path).unwrap(), accepted, "{}: rejection rewrote the file", sample.key);
        }
    }

    fn home_path(home: &TempHome, bot: bool) -> PathBuf {
        if bot {
            home.path.join("alpha/bot.toml")
        } else {
            home.path.join("herdbot.toml")
        }
    }

    #[test]
    fn rejected_values_name_the_setting_and_leave_the_bytes_unchanged() {
        let cases = [
            (None, "approvals.mode", "yolo", "one of \"ask\", \"auto\""),
            (None, "codex.sandbox", "full", "one of \"default\", \"none\""),
            (None, "memory.max_compactors", "two", "an integer >= 0"),
            (None, "memory.max_compactors", "-1", "an integer >= 0"),
            (None, "memory.max_compactors", "9223372036854775808", "an integer >= 0"),
            (None, "paths", "/workspace/vault", "FROM=TO"),
            (None, "paths", "=/vault", "a non-empty FROM path"),
            (None, "paths", "/workspace/vault=", "a non-empty TO path"),
            (None, "cua.host", "", "one non-empty word"),
            (None, "cua.command", "a b", "one non-empty word"),
            (Some("alpha"), "title", "two\nlines", "a single line"),
            (Some("alpha"), "cwd", "   ", "a non-empty path"),
            (Some("alpha"), "class", "not a class", "one non-empty word"),
            (Some("alpha"), "approvals", "yolo", "one of \"ask\", \"auto\""),
            (Some("alpha"), "memory", "yes", "true or false"),
            (Some("alpha"), "memory", "1", "true or false"),
            (Some("alpha"), "memory_view_bytes", "-5", "an integer >= 0"),
            (Some("alpha"), "memory_view_bytes", "lots", "an integer >= 0"),
            (None, "approvals", "auto", "unknown global setting"),
            (Some("alpha"), "approvals.mode", "auto", "unknown bot setting"),
            (None, "routing.default", "x", "routing.toml is not managed"),
        ];
        for (index, (bot, key, value, expected)) in cases.into_iter().enumerate() {
            let home = TempHome::new(&format!("reject-{index}"));
            let path = if bot.is_some() {
                home.write("alpha/bot.toml", "name = \"alpha\"\napprovals = \"ask\"\n")
            } else {
                home.write("herdbot.toml", "[approvals]\nmode = \"ask\"\n")
            };
            let before = std::fs::read(&path).unwrap();
            let error = set(&home.path, bot, key, value).unwrap_err().to_string();
            assert!(error.contains(expected), "{key} = {value:?}: {error}");
            assert_eq!(std::fs::read(&path).unwrap(), before, "{key} = {value:?} rewrote the file");
            assert_only(path.parent().unwrap(), &[path.file_name().unwrap().to_string_lossy().as_ref()]);
        }
    }

    #[test]
    fn set_preserves_comments_key_order_and_unrelated_sections() {
        let home = TempHome::new("preserve");
        let path = home.write(
            "herdbot.toml",
            "\
# herdbot settings\n\
# keep this comment\n\
\n\
[approvals]\n\
# why auto\n\
path = \"ignored\"\n\
mode = \"ask\"   # inline comment\n\
\n\
[paths]\n\
\"/workspace/obsidian-vault\" = \"~/Documents/vault\"\n\
\n\
# unrelated stays untouched\n\
[unrelated]\n\
future = true\n",
        );
        set(&home.path, None, "approvals.mode", "auto").unwrap();
        set(&home.path, None, "paths", "/workspace/repo=~/Code/repo").unwrap();
        set(&home.path, None, "memory.max_compactors", "5").unwrap();
        assert_eq!(
            read(&path),
            "\
# herdbot settings\n\
# keep this comment\n\
\n\
[approvals]\n\
# why auto\n\
path = \"ignored\"\n\
mode = \"auto\"   # inline comment\n\
\n\
[paths]\n\
\"/workspace/obsidian-vault\" = \"~/Documents/vault\"\n\
\"/workspace/repo\" = \"~/Code/repo\"\n\
\n\
# unrelated stays untouched\n\
[unrelated]\n\
future = true\n\
\n\
[memory]\n\
max_compactors = 5\n"
        );
    }

    #[test]
    fn bot_writes_keep_the_other_keys_comments_and_order() {
        let home = TempHome::new("preserve-bot");
        let path = home.write(
            "alpha/bot.toml",
            "\
# the bot\n\
name = \"alpha\"\n\
title = \"Alpha\"   # shown in listings\n\
memory = true\n\
\n\
[[schedules]]\n\
name = \"daily\"\n\
cron = \"0 9 * * *\"\n\
prompt = \"check\"\n",
        );
        set(&home.path, Some("alpha"), "cwd", "/Users/shane/Code/alpha").unwrap();
        set(&home.path, Some("alpha"), "approvals", "auto").unwrap();
        assert_eq!(
            read(&path),
            "\
# the bot\n\
name = \"alpha\"\n\
title = \"Alpha\"   # shown in listings\n\
memory = true\n\
cwd = \"/Users/shane/Code/alpha\"\n\
approvals = \"auto\"\n\
\n\
[[schedules]]\n\
name = \"daily\"\n\
cron = \"0 9 * * *\"\n\
prompt = \"check\"\n"
        );
    }

    #[test]
    fn set_creates_a_missing_section_and_a_missing_global_file() {
        let home = TempHome::new("create-section");
        let path = home.write("herdbot.toml", "unrelated = true\n");
        let setting = set(&home.path, None, "approvals.mode", "auto").unwrap();
        assert_eq!(setting.value, json!("auto"));
        assert_eq!(read(&path), "unrelated = true\n\n[approvals]\nmode = \"auto\"\n");

        let empty = TempHome::new("create-file");
        set(&empty.path, None, "approvals.mode", "auto").unwrap();
        assert_eq!(
            read(&empty.path.join("herdbot.toml")),
            "[approvals]\nmode = \"auto\"\n"
        );
    }

    #[test]
    fn inline_tables_keep_their_shape_and_inline_comment() {
        let home = TempHome::new("inline");
        let path = home.write(
            "herdbot.toml",
            "approvals = { mode = \"ask\" }  # pinned\n",
        );
        set(&home.path, None, "approvals.mode", "auto").unwrap();
        assert_eq!(read(&path), "approvals = { mode = \"auto\" }  # pinned\n");

        let bot = TempHome::new("inline-bot");
        let path = bot.write(
            "alpha/bot.toml",
            "name = \"alpha\"\napprovals = { mode = \"ask\" }\n",
        );
        // The bot key is a string, so a table there is repaired rather than edited.
        set(&bot.path, Some("alpha"), "approvals", "auto").unwrap();
        assert_eq!(read(&path), "name = \"alpha\"\napprovals = \"auto\"\n");

        // A dotted key with no section header keeps that shape in both directions.
        let dotted = TempHome::new("dotted");
        let path = dotted.write("herdbot.toml", "approvals.mode = \"ask\"\nother = true\n");
        set(&dotted.path, None, "approvals.mode", "auto").unwrap();
        assert_eq!(read(&path), "approvals.mode = \"auto\"\nother = true\n");
        unset(&dotted.path, None, "approvals.mode").unwrap();
        assert_eq!(read(&path), "other = true\n");
    }

    #[test]
    fn successful_writes_replace_the_file_atomically() {
        let home = TempHome::new("atomic");
        let path = home.write("herdbot.toml", "[approvals]\nmode = \"ask\"\n");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let before_inode = inode(&path);
        // A hard link keeps the old inode: it only survives if the rename replaced the directory
        // entry instead of truncating the file in place.
        let old = home.path.join("herdbot.toml.before");
        std::fs::hard_link(&path, &old).unwrap();

        set(&home.path, None, "approvals.mode", "auto").unwrap();

        assert_eq!(read(&path), "[approvals]\nmode = \"auto\"\n");
        assert_ne!(inode(&path), before_inode, "the file was rewritten in place");
        assert_eq!(read(&old), "[approvals]\nmode = \"ask\"\n");
        assert_eq!(mode(&path), 0o600, "the original permissions were not kept");
        assert_only(&home.path, &["herdbot.toml", "herdbot.toml.before"]);
    }

    #[test]
    fn a_failed_atomic_write_keeps_the_original_bytes_and_no_temp_file() {
        let home = TempHome::new("atomic-failure");
        let path = home.write("herdbot.toml", "[approvals]\nmode = \"ask\"\n");
        let error = write_atomic_with(&path, |file| {
            use std::io::Write;
            file.write_all(b"[approvals]\nmode = \"au")?;
            Err(std::io::Error::other("disk full"))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("disk full"), "{error}");
        assert!(error.contains(&path.to_string_lossy().to_string()), "{error}");
        assert_eq!(read(&path), "[approvals]\nmode = \"ask\"\n");
        assert_only(&home.path, &["herdbot.toml"]);
    }

    #[test]
    fn unset_removes_the_key_and_restores_the_default_without_touching_the_rest() {
        let home = TempHome::new("unset");
        let path = home.write(
            "herdbot.toml",
            "\
# top\n\
[approvals]\n\
# why auto\n\
mode = \"auto\"   # inline\n\
\n\
[cua]\n\
host = \"shane@studio.local\"\n",
        );
        let setting = unset(&home.path, None, "approvals.mode").unwrap();
        assert_eq!(setting.value, json!("ask"));
        assert_eq!(setting.default, json!("ask"));
        // The section header and its own comment stay; the comment above the removed key and
        // its inline comment go with the key.
        assert_eq!(
            read(&path),
            "\
# top\n\
[approvals]\n\
\n\
[cua]\n\
host = \"shane@studio.local\"\n"
        );

        assert_eq!(unset(&home.path, None, "cua.host").unwrap().value, Json::Null);
        assert_eq!(read(&path), "# top\n[approvals]\n\n[cua]\n", "emptied section headers stay");
    }

    #[test]
    fn unset_of_an_absent_key_writes_nothing() {
        let home = TempHome::new("unset-absent-key");
        let path = home.write("herdbot.toml", "[approvals]\nmode = \"ask\"\n");
        let before = std::fs::read(&path).unwrap();
        let before_inode = inode(&path);
        let setting = unset(&home.path, None, "cua.host").unwrap();
        assert_eq!(setting.value, Json::Null);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(inode(&path), before_inode, "an absent key rewrote the file");

        let missing = TempHome::new("unset-absent-file");
        assert_eq!(
            unset(&missing.path, None, "approvals.mode").unwrap().value,
            json!("ask")
        );
        assert!(!missing.path.join("herdbot.toml").exists());
    }

    #[test]
    fn unset_bot_approvals_falls_back_to_the_global_mode() {
        let home = TempHome::new("unset-inherit");
        home.write("herdbot.toml", "[approvals]\nmode = \"auto\"\n");
        let path = home.write(
            "alpha/bot.toml",
            "name = \"alpha\"\napprovals = \"auto\"  # pinned\nmemory = true\n",
        );
        let setting = unset(&home.path, Some("alpha"), "approvals").unwrap();
        assert_eq!(setting.value, json!("auto"), "the global mode now applies");
        assert_eq!(setting.default, json!("ask"));
        assert_eq!(read(&path), "name = \"alpha\"\nmemory = true\n");
    }

    #[test]
    fn unset_paths_removes_the_whole_table_and_set_restores_it() {
        let home = TempHome::new("unset-paths");
        let path = home.write("herdbot.toml", "unrelated = true\n");
        set(&home.path, None, "paths", "/workspace/a=~/a").unwrap();
        set(&home.path, None, "paths", "/workspace/b=~/b").unwrap();
        assert_eq!(
            get(&home.path, None, "paths").unwrap().value,
            json!({"/workspace/a": "~/a", "/workspace/b": "~/b"})
        );
        assert_eq!(unset(&home.path, None, "paths").unwrap().value, json!({}));
        assert_eq!(read(&path), "unrelated = true\n");
    }

    #[test]
    fn set_and_unset_reject_unsafe_bot_ids_and_unknown_bots() {
        let home = TempHome::new("write-bot-ids");
        let long = "a".repeat(21);
        for bad in ["../alpha", "a/b", "Alpha", "", ".", long.as_str()] {
            let error = set(&home.path, Some(bad), "title", "x").unwrap_err().to_string();
            assert!(error.contains("invalid bot id"), "{bad:?}: {error}");
            let error = unset(&home.path, Some(bad), "title").unwrap_err().to_string();
            assert!(error.contains("invalid bot id"), "{bad:?}: {error}");
        }
        let error = set(&home.path, Some("ghost"), "title", "x").unwrap_err().to_string();
        assert!(error.contains("no bot \"ghost\""), "{error}");
        assert_eq!(entries(&home.path), Vec::<String>::new(), "a rejected bot id created files");
    }

    #[test]
    fn writes_refuse_malformed_or_structurally_wrong_files() {
        let home = TempHome::new("write-malformed");
        let path = home.write("herdbot.toml", "not = [toml\n");
        let error = set(&home.path, None, "approvals.mode", "auto").unwrap_err().to_string();
        assert!(error.contains("is not valid TOML"), "{error}");
        assert_eq!(read(&path), "not = [toml\n");

        let wrong = home.write("herdbot.toml", "approvals = \"auto\"\n");
        let error = set(&home.path, None, "approvals.mode", "auto").unwrap_err().to_string();
        assert!(error.contains("not a table"), "{error}");
        assert_eq!(read(&wrong), "approvals = \"auto\"\n");

        let scalar = home.write("herdbot.toml", "paths = 3\n");
        let error = set(&home.path, None, "paths", "/w=/v").unwrap_err().to_string();
        assert!(error.contains("not a table"), "{error}");
        assert_eq!(read(&scalar), "paths = 3\n");
        // `unset` removes the key whatever it holds, so a bad value is repaired too.
        assert_eq!(unset(&home.path, None, "paths").unwrap().value, json!({}));
        assert_eq!(read(&scalar), "");
    }

    #[test]
    fn a_write_that_would_read_back_badly_never_lands() {
        let home = TempHome::new("write-bad-read");
        home.write("herdbot.toml", "[approvals]\nmode = \"yolo\"\n");
        let path = home.write("alpha/bot.toml", "name = \"alpha\"\napprovals = \"auto\"\n");
        let before = std::fs::read(&path).unwrap();
        // The bot would inherit an invalid global mode: the read-back check fails first.
        let error = unset(&home.path, Some("alpha"), "approvals")
            .unwrap_err()
            .to_string();
        assert!(error.contains("approvals.mode"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_only(&home.path.join("alpha"), &["bot.toml"]);
    }

    #[test]
    fn set_returns_the_versioned_json_contract() {
        let home = TempHome::new("set-json");
        home.write("herdbot.toml", "# keep\n[memory]\nmax_compactors = 2\n");
        let setting = set(&home.path, None, "memory.max_compactors", "6").unwrap();
        let report = serde_json::to_value(GetReport {
            version: CONTRACT_VERSION,
            scope: "global",
            setting: &setting,
        })
        .unwrap();
        assert_eq!(report["version"], json!(1));
        assert_eq!(report["scope"], json!("global"));
        assert_eq!(report["setting"]["key"], json!("memory.max_compactors"));
        assert_eq!(report["setting"]["type"], json!("integer"));
        assert_eq!(report["setting"]["value"], json!(6));
        assert_eq!(report["setting"]["default"], json!(2));
        assert_eq!(report["setting"]["apply"], json!("immediate"));
    }

    #[test]
    fn a_file_without_a_trailing_newline_keeps_that_shape_and_a_repeat_write_is_a_no_op() {
        let home = TempHome::new("no-trailing-newline");
        let path = home.write("herdbot.toml", "[approvals]\nmode = \"ask\"");
        let before_inode = inode(&path);
        set(&home.path, None, "approvals.mode", "ask").unwrap();
        assert_eq!(read(&path), "[approvals]\nmode = \"ask\"");
        assert_eq!(inode(&path), before_inode, "an identical write rewrote the file");

        set(&home.path, None, "approvals.mode", "auto").unwrap();
        assert_eq!(read(&path), "[approvals]\nmode = \"auto\"");
    }

    #[test]
    fn write_output_names_the_setting_file_and_apply_timing() {
        let home = TempHome::new("write-human");
        set(&home.path, None, "approvals.mode", "auto").unwrap();
        let path = home.path.join("herdbot.toml");
        let setting = get(&home.path, None, "approvals.mode").unwrap();
        let text = render_change("set", &path, &setting);
        assert!(text.starts_with("set approvals.mode in "));
        assert!(text.contains(&path.to_string_lossy().to_string()));
        assert!(text.contains("approvals.mode = \"auto\""));
        assert!(text.contains("default: \"ask\""));
        assert!(text.contains("applies: on the bot's next start"));

        let setting = unset(&home.path, None, "approvals.mode").unwrap();
        let text = render_change("unset", &path, &setting);
        assert!(text.starts_with("unset approvals.mode in "));
        assert!(text.contains("approvals.mode = \"ask\""));
    }
}
