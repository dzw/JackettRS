//! Mirror of `applyGoTemplateText` in CardigannIndexer.cs
//!
//! A pragmatic subset of the Go text/template engine used by Cardigann
//! definitions: re_replace, join, and/or/eq/ne logic, if/else, range and
//! simple variable substitution.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use super::definition::{IF_ELSE_REGEX, JOIN_REGEX, LOGIC_FUNCTION_REGEX, RANGE_REGEX, RE_REPLACE_REGEX, VARIABLES_REGEX};

/// A template variable value (C# used `object`: string, ICollection<string> or null).
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateValue {
    Str(Option<String>),
    List(Vec<String>),
}

impl TemplateValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TemplateValue::Str(Some(s)) => Some(s),
            _ => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            TemplateValue::Str(None) => true,
            TemplateValue::Str(Some(s)) => s.trim().is_empty(),
            TemplateValue::List(l) => l.is_empty(),
        }
    }

    pub fn render(&self) -> Option<String> {
        match self {
            TemplateValue::Str(s) => s.clone(),
            TemplateValue::List(l) => Some(l.join(",")),
        }
    }
}

pub type TemplateVars = HashMap<String, TemplateValue>;

impl From<&str> for TemplateValue {
    fn from(s: &str) -> Self {
        TemplateValue::Str(Some(s.to_string()))
    }
}

impl From<String> for TemplateValue {
    fn from(s: String) -> Self {
        TemplateValue::Str(Some(s))
    }
}

impl From<Option<String>> for TemplateValue {
    fn from(s: Option<String>) -> Self {
        TemplateValue::Str(s)
    }
}

impl From<Vec<String>> for TemplateValue {
    fn from(v: Vec<String>) -> Self {
        TemplateValue::List(v)
    }
}

/// Optional text modifier applied to expanded values
/// (C# `TemplateTextModifier` delegate — used for URL-encoding paths).
pub type TemplateTextModifier = fn(&str) -> String;

/// Mirror of `applyGoTemplateText`. Returns the template with all supported
/// constructs expanded.
pub fn apply_go_template_text(
    template: &str,
    variables: Option<&TemplateVars>,
    modifier: Option<TemplateTextModifier>,
) -> String {
    if template.trim().is_empty() || !template.contains("{{") {
        return template.to_string();
    }

    let vars = variables; // caller supplies base variables
    let mut out = template.to_string();

    // handle re_replace expression
    // Example: {{ re_replace .Query.Keywords "[^a-zA-Z0-9]+" "%" }}
    while let Some(m) = RE_REPLACE_REGEX.captures(&out) {
        let all = m.get(0).unwrap().as_str();
        let variable = m.get(1).unwrap().as_str();
        let regexp = m.get(2).unwrap().as_str();
        let newvalue = m.get(3).unwrap().as_str();

        let input = vars
            .and_then(|v| v.get(variable))
            .and_then(TemplateValue::as_str)
            .unwrap_or("");
        let replace_regex = Regex::new(regexp).unwrap_or(Regex::new(regexp.as_ref()).unwrap_or_else(|_| Regex::new("").unwrap()));
        let mut expanded = replace_regex.replace_all(input, newvalue.replace('$', "$").as_str()).to_string();

        if let Some(modifier) = modifier {
            expanded = modifier(&expanded);
        }

        out = out.replace(all, &expanded);
    }

    // handle join expression
    // Example: {{ join .Categories "," }}
    while let Some(m) = JOIN_REGEX.captures(&out) {
        let all = m.get(0).unwrap().as_str();
        let variable = m.get(1).unwrap().as_str();
        let delimiter = m.get(2).unwrap().as_str();

        let input: Vec<String> = match vars.and_then(|v| v.get(variable)) {
            Some(TemplateValue::List(l)) => l.clone(),
            Some(TemplateValue::Str(Some(s))) => vec![s.clone()],
            _ => Vec::new(),
        };
        let mut expanded = input.join(delimiter);

        if let Some(modifier) = modifier {
            expanded = modifier(&expanded);
        }

        out = out.replace(all, &expanded);
    }

    // handle logic functions and/or/eq/ne (repeated from the start so nested
    // functions like {{if and eq (.Var1) "x" eq (.Var2) "y"}} work)
    while let Some(m) = LOGIC_FUNCTION_REGEX.captures(&out) {
        let start = m.get(0).unwrap().start();
        let len = m.get(0).unwrap().len();
        let function_name = m.get(1).unwrap().as_str();
        // Group 2 is a repeated capture group — the `regex` crate only keeps
        // its LAST repetition, so scan the whole match (group 0) instead to
        // recover every `(.var)` / `"literal"` argument.
        let mut parameters: Vec<String> = m
            .get(0)
            .map(|g| {
                unreachable_repeated_group_workaround(g)
            })
            .unwrap_or_default();

        // Parentheses are Go template grouping syntax: `{{ or (.A) (.B) }}` is
        // equivalent to `{{ or .A .B }}`. Strip them so the argument resolves
        // against the variable table (keys are stored without parentheses) and
        // the or/and result re-enters the variable pass below.
        for parameter in parameters.iter_mut() {
            if parameter.starts_with('(') && parameter.ends_with(')') {
                *parameter = parameter[1..parameter.len() - 1].to_string();
            }
        }

        // If the function can't use string literals, fail silently by removing them.
        if function_name != "eq" && function_name != "ne" {
            parameters.retain(|param| !param.starts_with('"'));
        }

        let function_result = match function_name {
            // returns first null or empty, else last variable
            "and" | "or" => {
                let is_and = function_name == "and";
                let mut result = String::new();
                for parameter in &parameters {
                    result = parameter.clone();
                    let value = vars.and_then(|v| v.get(parameter));
                    let is_blank = value.map(TemplateValue::is_empty).unwrap_or(true);
                    if is_blank == is_and {
                        break;
                    }
                }
                result
            }
            // Returns .True if equal / .False otherwise
            "eq" | "ne" => {
                let want_equal = function_name == "eq";
                let is_equal = parameters
                    .iter()
                    .take(2)
                    .map(|param| {
                        if param.starts_with('"') {
                            param.trim_matches('"').to_string()
                        } else {
                            vars.and_then(|v| v.get(param))
                                .and_then(TemplateValue::as_str)
                                .unwrap_or("")
                                .to_string()
                        }
                    })
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == 1;
                if is_equal == want_equal {
                    ".True".to_string()
                } else {
                    ".False".to_string()
                }
            }
            _ => String::new(),
        };

        out.replace_range(start..start + len, &function_result);
    }

    // handle if ... else ... expression
    while let Some(m) = IF_ELSE_REGEX.captures(&out) {
        let all = m.get(0).unwrap().as_str();
        let condition = m.get(1).unwrap().as_str();
        let on_true = m.get(2).unwrap().as_str();
        let on_false = m.get(3).unwrap().as_str();

        // `eq`/`ne` above yield the boolean sentinels ".True"/".False"; treat
        // them as literals rather than variable lookups. Parenthesized
        // conditions (`{{ if (.Var) }}`) are grouping syntax — unwrap them.
        let condition = condition.trim();
        let condition = condition
            .strip_prefix('(')
            .and_then(|c| c.strip_suffix(')'))
            .map(str::trim)
            .unwrap_or(condition);

        let condition_result = if condition == ".True" {
            on_true.to_string()
        } else if condition == ".False" {
            on_false.to_string()
        } else if condition.starts_with('.') {
            let value = vars.and_then(|v| v.get(condition));
            let state = match value {
                None => false,
                Some(v) => !v.is_empty(),
            };
            if state {
                on_true.to_string()
            } else {
                on_false.to_string()
            }
        } else {
            // mirror of the NotImplementedException
            on_false.to_string()
        };

        out = out.replace(all, &condition_result);
    }

    // handle range expression
    while let Some(m) = RANGE_REGEX.captures(&out) {
        let all = m.get(0).unwrap().as_str();
        let index = m.name("index").map(|g| g.as_str()).unwrap_or("");
        let variable = m.name("variable").map(|g| g.as_str()).unwrap_or("");
        let prefix = m.name("prefix").map(|g| g.as_str()).unwrap_or("");
        let postfix = m.name("postfix").map(|g| g.as_str()).unwrap_or("");

        let mut expanded = String::new();
        let index_replace = format!("{{{{{}}}}}", index);

        let values: Vec<String> = match vars.and_then(|v| v.get(variable)) {
            Some(TemplateValue::List(l)) => l.clone(),
            Some(TemplateValue::Str(Some(s))) => vec![s.clone()],
            _ => Vec::new(),
        };

        for (array_index, value) in values.iter().enumerate() {
            let mut newvalue = value.clone();
            if let Some(modifier) = modifier {
                newvalue = modifier(&newvalue);
            }
            if !index.is_empty() {
                expanded += &prefix.replace(&index_replace, &array_index.to_string());
                expanded += &newvalue;
                expanded += &postfix.replace(&index_replace, &array_index.to_string());
            } else {
                expanded += prefix;
                expanded += &newvalue;
                expanded += postfix;
            }
        }

        out = out.replace(all, &expanded);
    }

    // handle simple variables
    while let Some(m) = VARIABLES_REGEX.captures(&out) {
        let all = m.get(0).unwrap().as_str();
        let variable = m.get(1).unwrap().as_str();

        let mut value = vars
            .and_then(|v| v.get(variable))
            .and_then(TemplateValue::render)
            .unwrap_or_default();
        if let Some(modifier) = modifier {
            value = modifier(&value);
        }

        out = out.replace(all, &value);
    }

    out
}

/// Extracts the repeated subgroup captures of a regex capture by re-matching
/// the same pattern over the matched span. The C# code used `Group.Captures`;
/// the Rust `regex` crate does not expose repeated-capture lists, so we scan.
fn unreachable_repeated_group_workaround(group: regex::Match<'_>) -> Vec<String> {
    static PARAM_SCAN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(\(?\.[^\)\s ]+\)?|"[^"]+")"#).unwrap());
    PARAM_SCAN
        .find_iter(group.as_str())
        .map(|mm| mm.as_str().to_string())
        .collect()
}

/// Mirror of `GetBaseTemplateVariables` — the always-available variables.
pub fn get_base_template_variables(
    site_link: &str,
    settings: &[crate::models::indexer_definition::SettingsField],
    config_values: &HashMap<String, TemplateValue>,
) -> TemplateVars {
    use chrono::Datelike;

    let mut variables = TemplateVars::new();
    variables.insert(".Config.sitelink".into(), site_link.into());
    variables.insert(".True".into(), "True".into());
    variables.insert(".False".into(), TemplateValue::Str(None));

    // .Today.Year: previous year in January, current year otherwise
    let today = chrono::Local::now();
    let year = if today.month() > 1 {
        today.year()
    } else {
        today.year() - 1
    };
    variables.insert(".Today.Year".into(), TemplateValue::from(year.to_string().as_str()));

    for setting in settings {
        let Some(name) = &setting.name else { continue };
        let Some(value) = config_values.get(name) else { continue };
        variables.insert(format!(".Config.{}", name), value.clone());
    }

    variables
}

/// Percent-encoding used for search-path templates (mirror of the C#
/// `WebUtility.UrlEncode` modifier).
pub fn url_encode_component(input: &str) -> String {
    let mut out = String::new();
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'!' | b'('
            | b')' | b'\'' | b'*' => out.push(*byte as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> TemplateVars {
        let mut v = TemplateVars::new();
        for (k, s) in pairs {
            v.insert(
                k.to_string(),
                if s.is_empty() {
                    TemplateValue::Str(None)
                } else {
                    TemplateValue::Str(Some(s.to_string()))
                },
            );
        }
        v
    }

    /// nyaasi `title_phase2`: `{{ or (.Result.title_phase1) (.Result.title_default) }}`
    /// — the optional title_phase1 is absent, so title_default must win.
    #[test]
    fn or_with_parenthesized_args_resolves() {
        let v = vars(&[(".Result.title_phase1", ""), (".Result.title_default", "HoneyContrast FLAC")]);
        let out = apply_go_template_text(
            "{{ or (.Result.title_phase1) (.Result.title_default) }}",
            Some(&v),
            None,
        );
        assert_eq!(out, "HoneyContrast FLAC");
    }

    #[test]
    fn or_prefers_first_non_empty() {
        let v = vars(&[(".A", "first"), (".B", "second")]);
        let out = apply_go_template_text("{{ or (.A) (.B) }}", Some(&v), None);
        assert_eq!(out, "first");
    }

    #[test]
    fn and_returns_last_when_all_present() {
        let v = vars(&[(".A", "x"), (".B", "y")]);
        let out = apply_go_template_text("{{ and (.A) (.B) }}", Some(&v), None);
        assert_eq!(out, "y");
    }

    #[test]
    fn eq_with_parenthesized_args() {
        let v = vars(&[(".A", "same"), (".B", "same")]);
        let out = apply_go_template_text("{{ if eq (.A) (.B) }}yes{{ else }}no{{ end }}", Some(&v), None);
        assert_eq!(out, "yes");
    }
}
