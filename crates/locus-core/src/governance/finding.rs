//! `RuleFinding` and the evidence it carries.
//!
//! Findings are the substrate that policies decide over. A finding is
//! emitted by either a registered rule (`FindingSource::RegisteredRule`),
//! the legacy compat adapter (`FindingSource::LegacyDiagnostic`), or a
//! policy itself (`FindingSource::Policy`).

// locus: ot canonical

use crate::diagnostics::Severity;
use crate::governance::ids::{FindingId, ParadigmId, PolicyId, RuleId};
use locus_air::AirSpan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Evidence {
    /// Typed variant for complexity-budget rules (CX001/CX002).
    ComplexityBudget {
        lines: u32,
        budget: u32,
        override_match: Option<String>,
    },
    /// Typed variant for inference-shaped ownership rules (OT002, etc.).
    InferenceConfidence {
        score: Confidence,
        signals: Vec<String>,
    },
    /// Catch-all for migrated rules whose schema is not yet typed.
    Structured(serde_json::Value),
    /// Adapter payload — original diagnostic prose, no schema.
    Legacy(LegacyEvidence),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyEvidence {
    pub original_message: String,
    pub original_why: Vec<String>,
    pub original_suggested_fix: Option<String>,
}

// Serialization wrappers for ID types with &'static str.
// These are needed because RuleId/ParadigmId/PolicyId hold `&'static str`,
// which serde cannot deserialize (the lifetime may not be `'static`).
// We wrap them in newtypes that serialize to/from strings and use
// Box::leak to convert deserialized strings back to `&'static str`.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
struct RuleIdSerde(String);

impl From<RuleId> for RuleIdSerde {
    fn from(id: RuleId) -> Self {
        RuleIdSerde(id.as_str().to_string())
    }
}

impl From<RuleIdSerde> for RuleId {
    fn from(serde: RuleIdSerde) -> Self {
        RuleId::new(Box::leak(serde.0.into_boxed_str()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
struct ParadigmIdSerde(String);

impl From<ParadigmId> for ParadigmIdSerde {
    fn from(id: ParadigmId) -> Self {
        ParadigmIdSerde(id.as_str().to_string())
    }
}

impl From<ParadigmIdSerde> for ParadigmId {
    fn from(serde: ParadigmIdSerde) -> Self {
        ParadigmId::new(Box::leak(serde.0.into_boxed_str()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
struct PolicyIdSerde(String);

impl From<PolicyId> for PolicyIdSerde {
    fn from(id: PolicyId) -> Self {
        PolicyIdSerde(id.as_str().to_string())
    }
}

impl From<PolicyIdSerde> for PolicyId {
    fn from(serde: PolicyIdSerde) -> Self {
        PolicyId::new(Box::leak(serde.0.into_boxed_str()))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FindingSource {
    RegisteredRule(RuleId),
    LegacyDiagnostic {
        rule_code: String,
        paradigm: Option<ParadigmId>,
    },
    Policy(PolicyId),
}

impl Serialize for FindingSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::{SerializeTupleVariant, SerializeStructVariant};
        match self {
            FindingSource::RegisteredRule(id) => {
                let mut v = serializer.serialize_tuple_variant("FindingSource", 0, "RegisteredRule", 1)?;
                v.serialize_field(id.as_str())?;
                v.end()
            }
            FindingSource::LegacyDiagnostic { rule_code, paradigm } => {
                let mut v = serializer.serialize_struct_variant("FindingSource", 1, "LegacyDiagnostic", 2)?;
                v.serialize_field("rule_code", rule_code)?;
                v.serialize_field(
                    "paradigm",
                    &paradigm.as_ref().map(|p| p.as_str()),
                )?;
                v.end()
            }
            FindingSource::Policy(id) => {
                let mut v = serializer.serialize_tuple_variant("FindingSource", 2, "Policy", 1)?;
                v.serialize_field(id.as_str())?;
                v.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for FindingSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, EnumAccess, VariantAccess, Visitor};
        use std::fmt;

        struct FindingSourceVisitor;

        impl<'de> Visitor<'de> for FindingSourceVisitor {
            type Value = FindingSource;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("enum FindingSource")
            }

            fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
            where
                A: EnumAccess<'de>,
            {
                let (variant, content) = data.variant()?;
                match variant {
                    "RegisteredRule" => {
                        let s = content.tuple_variant(1, StringVisitor)?;
                        Ok(FindingSource::RegisteredRule(
                            RuleId::new(Box::leak(s.into_boxed_str())),
                        ))
                    }
                    "LegacyDiagnostic" => {
                        let (rule_code, paradigm) = content.struct_variant(
                            &["rule_code", "paradigm"],
                            LegacyDiagnosticVisitor,
                        )?;
                        Ok(FindingSource::LegacyDiagnostic {
                            rule_code,
                            paradigm: paradigm
                                .map(|p| ParadigmId::new(Box::leak(p.into_boxed_str()))),
                        })
                    }
                    "Policy" => {
                        let s = content.tuple_variant(1, StringVisitor)?;
                        Ok(FindingSource::Policy(
                            PolicyId::new(Box::leak(s.into_boxed_str())),
                        ))
                    }
                    _ => Err(de::Error::unknown_variant(
                        variant,
                        &["RegisteredRule", "LegacyDiagnostic", "Policy"],
                    )),
                }
            }
        }

        struct StringVisitor;

        impl<'de> Visitor<'de> for StringVisitor {
            type Value = String;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                seq.next_element()?
                    .ok_or_else(|| serde::de::Error::custom("expected string in tuple"))
            }
        }

        struct LegacyDiagnosticVisitor;

        impl<'de> Visitor<'de> for LegacyDiagnosticVisitor {
            type Value = (String, Option<String>);

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct LegacyDiagnostic")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut rule_code = None;
                let mut paradigm = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "rule_code" => rule_code = Some(map.next_value()?),
                        "paradigm" => paradigm = Some(map.next_value()?),
                        _ => {
                            let _ = map.next_value::<serde_json::Value>()?;
                        }
                    }
                }
                Ok((
                    rule_code.ok_or_else(|| serde::de::Error::missing_field("rule_code"))?,
                    paradigm,
                ))
            }
        }

        deserializer.deserialize_enum(
            "FindingSource",
            &["RegisteredRule", "LegacyDiagnostic", "Policy"],
            FindingSourceVisitor,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleFinding {
    pub id: FindingId,
    pub source: FindingSource,
    pub rule_id: Option<RuleId>,
    pub paradigm_id: Option<ParadigmId>,
    pub default_severity: Severity,
    pub span: Option<AirSpan>,
    pub concept: Option<String>,
    pub message: String,
    pub evidence: Vec<Evidence>,
    pub why: Vec<String>,
    pub suggested_fix: Option<String>,
    /// Governance/policy diagnostic code to emit when distinct from
    /// rule_id / source (e.g. `"LOCUS003"` for RegistryIntegrityPolicy
    /// findings). Resolved against `GovernanceDiagnosticRegistry`.
    pub diagnostic_code: Option<String>,
}

impl Serialize for RuleFinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("RuleFinding", 12)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("source", &self.source)?;
        state.serialize_field("rule_id", &self.rule_id.as_ref().map(|id| id.as_str()))?;
        state.serialize_field("paradigm_id", &self.paradigm_id.as_ref().map(|id| id.as_str()))?;
        state.serialize_field("default_severity", &self.default_severity)?;
        state.serialize_field("span", &self.span)?;
        state.serialize_field("concept", &self.concept)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("evidence", &self.evidence)?;
        state.serialize_field("why", &self.why)?;
        state.serialize_field("suggested_fix", &self.suggested_fix)?;
        state.serialize_field("diagnostic_code", &self.diagnostic_code)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for RuleFinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Visitor, MapAccess};
        use std::fmt;

        struct RuleFindingVisitor;

        impl<'de> Visitor<'de> for RuleFindingVisitor {
            type Value = RuleFinding;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct RuleFinding")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut id = None;
                let mut source = None;
                let mut rule_id = None;
                let mut paradigm_id = None;
                let mut default_severity = None;
                let mut span = None;
                let mut concept = None;
                let mut message = None;
                let mut evidence = None;
                let mut why = None;
                let mut suggested_fix = None;
                let mut diagnostic_code = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "id" => id = Some(map.next_value()?),
                        "source" => source = Some(map.next_value()?),
                        "rule_id" => {
                            let opt: Option<String> = map.next_value()?;
                            rule_id = opt.map(|s| RuleId::new(Box::leak(s.into_boxed_str())));
                        }
                        "paradigm_id" => {
                            let opt: Option<String> = map.next_value()?;
                            paradigm_id = opt.map(|s| ParadigmId::new(Box::leak(s.into_boxed_str())));
                        }
                        "default_severity" => default_severity = Some(map.next_value()?),
                        "span" => span = map.next_value()?,
                        "concept" => concept = map.next_value()?,
                        "message" => message = Some(map.next_value()?),
                        "evidence" => evidence = Some(map.next_value()?),
                        "why" => why = Some(map.next_value()?),
                        "suggested_fix" => suggested_fix = map.next_value()?,
                        "diagnostic_code" => diagnostic_code = map.next_value()?,
                        _ => {
                            let _ = map.next_value::<serde_json::Value>()?;
                        }
                    }
                }

                Ok(RuleFinding {
                    id: id.ok_or_else(|| de::Error::missing_field("id"))?,
                    source: source.ok_or_else(|| de::Error::missing_field("source"))?,
                    rule_id,
                    paradigm_id,
                    default_severity: default_severity
                        .ok_or_else(|| de::Error::missing_field("default_severity"))?,
                    span,
                    concept,
                    message: message.ok_or_else(|| de::Error::missing_field("message"))?,
                    evidence: evidence.ok_or_else(|| de::Error::missing_field("evidence"))?,
                    why: why.ok_or_else(|| de::Error::missing_field("why"))?,
                    suggested_fix,
                    diagnostic_code,
                })
            }
        }

        deserializer.deserialize_struct(
            "RuleFinding",
            &[
                "id",
                "source",
                "rule_id",
                "paradigm_id",
                "default_severity",
                "span",
                "concept",
                "message",
                "evidence",
                "why",
                "suggested_fix",
                "diagnostic_code",
            ],
            RuleFindingVisitor,
        )
    }
}

/// Id-keyed store of all findings produced in a pipeline run. Backed by a
/// BTreeMap to keep iteration deterministic (insertion order matches id
/// order, since FindingIdMinter is sequential).
#[derive(Debug, Default)]
pub struct FindingStore {
    findings: BTreeMap<FindingId, RuleFinding>,
}

impl FindingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, f: RuleFinding) {
        self.findings.insert(f.id, f);
    }

    pub fn get(&self, id: FindingId) -> Option<&RuleFinding> {
        self.findings.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &RuleFinding> {
        self.findings.values()
    }

    pub fn len(&self) -> usize {
        self.findings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complexity_budget_round_trips_through_json() {
        let e = Evidence::ComplexityBudget {
            lines: 73,
            budget: 50,
            override_match: Some("custom/path".to_string()),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn confidence_carries_through_inference_evidence() {
        let e = Evidence::InferenceConfidence {
            score: Confidence::High,
            signals: vec!["matched canonical suffix".into()],
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn legacy_evidence_preserves_original_diagnostic_fields() {
        let le = LegacyEvidence {
            original_message: "X".into(),
            original_why: vec!["a".into(), "b".into()],
            original_suggested_fix: None,
        };
        let e = Evidence::Legacy(le.clone());
        match e {
            Evidence::Legacy(out) => assert_eq!(out, le),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn finding_store_returns_findings_in_id_order() {
        let mut store = FindingStore::new();
        let f1 = sample_finding(FindingId::from_raw_for_test(2), "MSG2");
        let f0 = sample_finding(FindingId::from_raw_for_test(0), "MSG0");
        let f1b = sample_finding(FindingId::from_raw_for_test(1), "MSG1");
        store.insert(f1);
        store.insert(f0);
        store.insert(f1b);
        let messages: Vec<&str> = store.iter().map(|f| f.message.as_str()).collect();
        assert_eq!(messages, vec!["MSG0", "MSG1", "MSG2"]);
    }

    #[test]
    fn finding_round_trips_through_json() {
        let f = sample_finding(FindingId::from_raw_for_test(7), "ok");
        let json = serde_json::to_string(&f).unwrap();
        let back: RuleFinding = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }

    fn sample_finding(id: FindingId, msg: &str) -> RuleFinding {
        RuleFinding {
            id,
            source: FindingSource::RegisteredRule(RuleId::new("CX001")),
            rule_id: Some(RuleId::new("CX001")),
            paradigm_id: Some(ParadigmId::new("CX")),
            default_severity: Severity::Warning,
            span: Some(AirSpan::new("src/foo.rs", 1, 1)),
            concept: None,
            message: msg.to_string(),
            evidence: Vec::new(),
            why: Vec::new(),
            suggested_fix: None,
            diagnostic_code: None,
        }
    }
}
