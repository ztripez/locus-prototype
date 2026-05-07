//! Architecture Intermediate Representation.
//!
//! Pure data + serde. No language-specific concerns, no inference logic.
//! Language adapters (e.g. `locus-rust`) build these structures; `locus-core`
//! consumes them. Schema is versioned via [`AirWorkspace::schema_version`] —
//! bump on any breaking field change.
//!
//! Self-application: every AIR type below is `// ot: canonical`. They are the
//! one accepted representation of "source facts in a workspace." No shadow
//! variants of these types should exist anywhere in the Locus codebase.

use serde::{Deserialize, Serialize};

/// AIR schema version. Bumped on breaking changes to how facts are emitted.
///
/// History:
/// - **1**: initial Phase 1 emission.
/// - **2**: type-text strings (`AirField.type_text`, `AirFunction.params`/
///   `return_type`, `AirConversion.from`/`to`, `AirConversion.symbol`) are
///   rendered cleanly — no extra spaces inside generics or around `&` / `::`.
/// - **3**: symbols are package-prefixed (`sample_crate::identity::User`)
///   instead of using the literal `crate` prefix. This makes symbols globally
///   unique across a Cargo workspace; without it, two crates can both emit
///   `crate::user::User` and collide in the lockfile.
pub const AIR_SCHEMA_VERSION: u32 = 3;

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirWorkspace {
    pub schema_version: u32,
    pub packages: Vec<AirPackage>,
}

impl AirWorkspace {
    pub fn new(packages: Vec<AirPackage>) -> Self {
        Self {
            schema_version: AIR_SCHEMA_VERSION,
            packages,
        }
    }
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirPackage {
    pub name: String,
    pub version: String,
    pub root_dir: String,
    pub files: Vec<AirFile>,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirFile {
    pub path: String,
    pub module_path: Option<String>,
    pub items: Vec<AirItem>,
    pub hints: Vec<AirHint>,
    pub parse_error: Option<String>,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AirItem {
    Type(AirType),
    Function(AirFunction),
    Conversion(AirConversion),
    Usage(AirUsage),
    TruthAction(AirTruthAction),
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirType {
    // Renamed in JSON to avoid colliding with the AirItem external tag (also `kind`).
    #[serde(rename = "type_kind")]
    pub kind: TypeKind,
    pub name: String,
    pub symbol: String,
    pub visibility: Visibility,
    pub fields: Vec<AirField>,
    pub variants: Vec<AirVariant>,
    pub derives: Vec<String>,
    pub attrs: Vec<String>,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TypeKind {
    Struct,
    Enum,
    Alias,
    Union,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirField {
    pub name: String,
    pub type_text: String,
    pub visibility: Visibility,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirVariant {
    pub name: String,
    pub fields: Vec<AirField>,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Crate,
    Restricted,
    Private,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirFunction {
    pub name: String,
    pub symbol: String,
    pub visibility: Visibility,
    pub params: Vec<(String, String)>,
    pub return_type: Option<String>,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirConversion {
    pub from: String,
    pub to: String,
    pub mechanism: ConversionMechanism,
    pub symbol: String,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConversionMechanism {
    From,
    TryFrom,
    InherentMethod,
    FreeFn,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirUsage {
    pub from_symbol: String,
    pub to_symbol: String,
    #[serde(rename = "usage_kind")]
    pub kind: UsageKind,
    pub span: AirSpan,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UsageKind {
    FunctionParam,
    FunctionReturn,
    FieldType,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirTruthAction {
    pub action: ActionKind,
    pub target: String,
    pub function: Option<String>,
    pub span: AirSpan,
    pub confidence: f32,
    pub reasons: Vec<String>,
}

// ot: canonical
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionKind {
    Construct,
    EnumMatch,
    StringCompare,
    Validate,
    Normalize,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirHint {
    pub kind: HintKind,
    pub raw: String,
    pub span: AirSpan,
    pub target_span: Option<AirSpan>,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "category", rename_all = "kebab-case")]
pub enum HintKind {
    Canonical,
    Boundary {
        concept: Option<String>,
        boundary: Option<String>,
    },
    Converter,
    ProtocolTranslation {
        reason: Option<String>,
    },
    GeneratedBoundary,
    Allow {
        rule: String,
        reason: Option<String>,
        expires: Option<String>,
    },
    Unknown,
}

// ot: canonical
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AirSpan {
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
}

impl AirSpan {
    pub fn new(file: impl Into<String>, line_start: u32, line_end: u32) -> Self {
        Self {
            file: file.into(),
            line_start,
            line_end,
        }
    }
}
