use crate::rule::Rule;
use crate::get_absolute_program_path;
use regex::Regex;
use reqwest::Url;
use serde::de::{Error, Unexpected, Visitor};
use serde::{Deserialize, Deserializer};
use serde_with::serde_as;
use std::fmt::{Debug, Display, Formatter};
use std::path::Path;

pub struct Pattern(pub Regex);

struct PatternVisitor;

fn parse_pattern<E>(str: &str) -> Result<Pattern, E>
where
    E: Error
{
    if str.is_empty() {
        return Err(E::invalid_length(0, &"non-empty regex"));
    }
    match Regex::new(str) {
        Ok(regex) => Ok(Pattern(regex)),
        Err(err) => Err(E::invalid_value(Unexpected::Str(err.to_string().as_str()), &"a valid regex"))
    }
}

impl<'de> Visitor<'de> for PatternVisitor {
    type Value = Pattern;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a valid regex")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error
    {
        parse_pattern(v)
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: Error
    {
        parse_pattern(v)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: Error
    {
        parse_pattern(v.as_str())
    }
}

impl <'de> Deserialize<'de> for Pattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>
    {
        deserializer.deserialize_str(PatternVisitor)
    }

    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: Deserializer<'de>
    {
        match deserializer.deserialize_str(PatternVisitor) {
            Ok(pattern) => {
                *place = pattern;
                Ok(())
            }
            Err(err) => Err(err)
        }
    }
}

impl Debug for Pattern {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

pub struct URL(pub Url);

struct URLVisitor;

fn parse_url<E>(str: &str) -> Result<URL, E>
where
    E: Error
{
    if str.is_empty() {
        return Err(E::invalid_length(0, &"non-empty regex"));
    }
    match Url::parse(str) {
        Ok(url) => Ok(URL(url)),
        Err(err) => Err(E::invalid_value(Unexpected::Str(err.to_string().as_str()), &"a valid URL"))
    }
}

impl<'de> Visitor<'de> for URLVisitor {
    type Value = URL;

    fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
        formatter.write_str("a valid URL")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error
    {
        parse_url(v)
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: Error
    {
        parse_url(v)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: Error
    {
        parse_url(v.as_str())
    }
}

impl <'de> Deserialize<'de> for URL {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>
    {
        deserializer.deserialize_str(URLVisitor)
    }

    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: Deserializer<'de>
    {
        match deserializer.deserialize_str(URLVisitor) {
            Ok(url) => {
                *place = url;
                Ok(())
            }
            Err(err) => Err(err)
        }
    }
}

impl Debug for URL {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

pub enum HookType {
    PreReceive,
    Update,
    PostReceive,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct HookBypass {
    pub push_option: String,
    pub messages: Option<Vec<String>>,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Hook {
    pub rule: Rule,
    pub reject_on_error: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConfigurationVersion1 {
    pub pre_receive: Option<Hook>,
    pub post_receive: Option<Hook>,
    pub update: Option<Hook>,
    pub bypass: Option<HookBypass>,
    pub trace: Option<bool>,
}

impl ConfigurationVersion1 {
    pub(crate) fn trace<T: Display>(&self, line: T, depth: u8) {
        if self.trace.unwrap_or(false) {
            eprintln!("trace: {}> {}", "-".repeat(depth.into()), line);
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "version")]
pub enum Configuration {
    #[serde(rename = "1")]
    Version1(ConfigurationVersion1)
}

impl ConfigurationVersion1 {
    pub fn select_hook(&self) -> Option<(&Hook, HookType)> {
        let exe_path = match get_absolute_program_path() {
            Ok(path) => path,
            Err(_) => return None
        };
        let by_name = hook_by_executable_name(&self, &exe_path);
        if by_name.is_some() {
            return by_name;
        }

        let by_parent = hook_by_parent_dir_name(&self, &exe_path);
        if by_parent.is_some() {
            return by_parent;
        }

        None
    }
}

fn hook_by_executable_name<'a>(configuration: &'a ConfigurationVersion1, path: &Path) -> Option<(&'a Hook, HookType)> {
    match path.file_name().and_then(|f| f.to_str()) {
        Some(name) => hook_by_name(configuration, name),
        None => None
    }
}

fn hook_by_parent_dir_name<'a>(configuration: &'a ConfigurationVersion1, path: &Path) -> Option<(&'a Hook, HookType)> {
    match path.parent().and_then(|f| f.file_name()).and_then(|f| f.to_str()) {
        Some(name) => hook_by_name(configuration, name.trim_end_matches(".d")),
        None => None
    }
}

fn hook_by_name<'a>(configuration: &'a ConfigurationVersion1, name: &str) -> Option<(&'a Hook, HookType)> {
    match name {
        "pre-receive" => {
            match &configuration.pre_receive {
                Some(h) => Some((h, HookType::PreReceive)),
                None => None
            }
        },
        "update" => {
            match &configuration.update {
                Some(h) => Some((h, HookType::Update)),
                None => None
            }
        },
        "post-receive" => {
            match &configuration.post_receive {
                Some(h) => Some((h, HookType::PostReceive)),
                None => None
            }
        },
        _ => None,
    }
}
