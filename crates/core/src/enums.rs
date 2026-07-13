// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

//! 配置文件中的枚举类型定义
//!
//! 本模块定义了所有配置文件中使用的枚举值，避免代码中出现字符串字面量。

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// 文件编码
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encoding {
    /// UTF-8（默认）
    #[serde(rename = "utf-8")]
    Utf8,
    /// UTF-8 with BOM
    #[serde(rename = "utf-8-bom")]
    Utf8Bom,
    /// UTF-16 Little Endian
    #[serde(rename = "utf-16le")]
    Utf16Le,
    /// UTF-16 Big Endian
    #[serde(rename = "utf-16be")]
    Utf16Be,
    /// GB2312（简体中文）
    #[serde(rename = "gb2312")]
    Gb2312,
    /// GBK（简体中文扩展）
    #[serde(rename = "gbk")]
    Gbk,
    /// Big5（繁体中文）
    #[serde(rename = "big5")]
    Big5,
}

impl Default for Encoding {
    fn default() -> Self {
        Self::Utf8
    }
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8 => write!(f, "utf-8"),
            Self::Utf8Bom => write!(f, "utf-8-bom"),
            Self::Utf16Le => write!(f, "utf-16le"),
            Self::Utf16Be => write!(f, "utf-16be"),
            Self::Gb2312 => write!(f, "gb2312"),
            Self::Gbk => write!(f, "gbk"),
            Self::Big5 => write!(f, "big5"),
        }
    }
}

impl FromStr for Encoding {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "utf-8" | "utf8" => Ok(Self::Utf8),
            "utf-8-bom" | "utf8-bom" => Ok(Self::Utf8Bom),
            "utf-16le" | "utf16le" => Ok(Self::Utf16Le),
            "utf-16be" | "utf16be" => Ok(Self::Utf16Be),
            "gb2312" => Ok(Self::Gb2312),
            "gbk" => Ok(Self::Gbk),
            "big5" => Ok(Self::Big5),
            _ => Err(format!("Unknown encoding: {}", s)),
        }
    }
}

impl Encoding {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf8Bom => "utf-8-bom",
            Self::Utf16Le => "utf-16le",
            Self::Utf16Be => "utf-16be",
            Self::Gb2312 => "gb2312",
            Self::Gbk => "gbk",
            Self::Big5 => "big5",
        }
    }
}

/// 行尾符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    /// LF (Unix/Linux/macOS) - \n（默认）
    #[serde(rename = "lf")]
    Lf,
    /// CRLF (Windows) - \r\n
    #[serde(rename = "crlf")]
    CrLf,
    /// CR (旧版 Mac) - \r
    #[serde(rename = "cr")]
    Cr,
}

impl Default for LineEnding {
    fn default() -> Self {
        Self::Lf
    }
}

impl fmt::Display for LineEnding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lf => write!(f, "lf"),
            Self::CrLf => write!(f, "crlf"),
            Self::Cr => write!(f, "cr"),
        }
    }
}

impl FromStr for LineEnding {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "lf" | "\n" => Ok(Self::Lf),
            "crlf" | "\r\n" => Ok(Self::CrLf),
            "cr" | "\r" => Ok(Self::Cr),
            _ => Err(format!("Unknown line ending: {}", s)),
        }
    }
}

impl LineEnding {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::CrLf => "crlf",
            Self::Cr => "cr",
        }
    }

    /// 返回实际的行尾字符
    pub fn as_bytes(&self) -> &'static [u8] {
        match self {
            Self::Lf => b"\n",
            Self::CrLf => b"\r\n",
            Self::Cr => b"\r",
        }
    }
}

/// JSON 空值表达方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JsonEmptyAs {
    /// 输出为 JSON null（默认）
    #[serde(rename = "null")]
    Null,
    /// 省略该字段
    #[serde(rename = "omit")]
    Omit,
}

impl Default for JsonEmptyAs {
    fn default() -> Self {
        Self::Null
    }
}

impl fmt::Display for JsonEmptyAs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Omit => write!(f, "omit"),
        }
    }
}

impl FromStr for JsonEmptyAs {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "null" => Ok(Self::Null),
            "omit" => Ok(Self::Omit),
            _ => Err(format!("Unknown JSON empty_as value: {}", s)),
        }
    }
}

impl JsonEmptyAs {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Omit => "omit",
        }
    }
}

/// XML 空值表达方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum XmlEmptyAs {
    /// 输出空标签 <field></field>（默认）
    #[serde(rename = "empty")]
    Empty,
    /// 不写入空字段
    #[serde(rename = "omit")]
    Omit,
}

impl Default for XmlEmptyAs {
    fn default() -> Self {
        Self::Empty
    }
}

impl fmt::Display for XmlEmptyAs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty"),
            Self::Omit => write!(f, "omit"),
        }
    }
}

impl FromStr for XmlEmptyAs {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "empty" => Ok(Self::Empty),
            "omit" => Ok(Self::Omit),
            _ => Err(format!("Unknown XML empty_as value: {}", s)),
        }
    }
}

impl XmlEmptyAs {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Omit => "omit",
        }
    }
}

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[serde(rename = "trace")]
    Trace,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "error")]
    Error,
}

impl Default for LogLevel {
    fn default() -> Self {
        Self::Info
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Trace => write!(f, "trace"),
            Self::Debug => write!(f, "debug"),
            Self::Info => write!(f, "info"),
            Self::Warn => write!(f, "warn"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            _ => Err(format!("Unknown log level: {}", s)),
        }
    }
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Picker 触发方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PickerTrigger {
    /// 单击触发
    #[serde(rename = "single")]
    Single,
    /// 双击触发
    #[serde(rename = "double")]
    Double,
}

impl Default for PickerTrigger {
    fn default() -> Self {
        Self::Double
    }
}

impl fmt::Display for PickerTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single => write!(f, "single"),
            Self::Double => write!(f, "double"),
        }
    }
}

impl FromStr for PickerTrigger {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "single" => Ok(Self::Single),
            "double" => Ok(Self::Double),
            _ => Err(format!("Unknown picker trigger: {}", s)),
        }
    }
}

impl PickerTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Double => "double",
        }
    }
}

/// 引用选择器列展示策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RefPickerStrategy {
    /// 自动：id + 最多 2 个辅助列（默认）
    #[serde(rename = "auto")]
    Auto,
    /// 完整：全部字段
    #[serde(rename = "full")]
    Full,
}

impl Default for RefPickerStrategy {
    fn default() -> Self {
        Self::Auto
    }
}

impl fmt::Display for RefPickerStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => write!(f, "auto"),
            Self::Full => write!(f, "full"),
        }
    }
}

impl FromStr for RefPickerStrategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "full" => Ok(Self::Full),
            _ => Err(format!("Unknown ref picker strategy: {}", s)),
        }
    }
}

impl RefPickerStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Full => "full",
        }
    }
}

/// C++ JSON 库选择
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CppJsonLib {
    /// nlohmann/json（默认）
    Nlohmann,
    /// RapidJSON
    RapidJson,
}

impl Default for CppJsonLib {
    fn default() -> Self {
        Self::Nlohmann
    }
}

impl fmt::Display for CppJsonLib {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nlohmann => write!(f, "nlohmann"),
            Self::RapidJson => write!(f, "rapidjson"),
        }
    }
}

impl FromStr for CppJsonLib {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "nlohmann" => Ok(Self::Nlohmann),
            "rapidjson" | "rapid" => Ok(Self::RapidJson),
            _ => Err(format!("Unknown C++ JSON library: {}", s)),
        }
    }
}

impl CppJsonLib {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Nlohmann => "nlohmann",
            Self::RapidJson => "rapidjson",
        }
    }
}

/// 项目排序方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectSort {
    /// 按 ID 排序（默认）
    #[serde(rename = "id")]
    Id,
    /// 按名称排序
    #[serde(rename = "name")]
    Name,
    /// 按打开状态排序
    #[serde(rename = "open")]
    Open,
    /// 按创建时间排序
    #[serde(rename = "created")]
    Created,
    /// 手动排序
    #[serde(rename = "manual")]
    Manual,
}

impl Default for ProjectSort {
    fn default() -> Self {
        Self::Id
    }
}

impl fmt::Display for ProjectSort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id => write!(f, "id"),
            Self::Name => write!(f, "name"),
            Self::Open => write!(f, "open"),
            Self::Created => write!(f, "created"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

impl FromStr for ProjectSort {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "id" => Ok(Self::Id),
            "name" => Ok(Self::Name),
            "open" => Ok(Self::Open),
            "created" => Ok(Self::Created),
            "manual" => Ok(Self::Manual),
            _ => Err(format!("Unknown project sort: {}", s)),
        }
    }
}

impl ProjectSort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::Open => "open",
            Self::Created => "created",
            Self::Manual => "manual",
        }
    }
}