use crate::DomainError;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

macro_rules! string_value {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                if value.trim().is_empty() || value.trim() != value {
                    return Err(DomainError::invalid_value(concat!(
                        stringify!($name),
                        " cannot be empty or padded with whitespace"
                    )));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

string_value!(ConnectionId);
string_value!(ResourceId);
string_value!(ResourceVersionId);
string_value!(RelationId);
string_value!(RelationVersionId);
string_value!(BindingId);
string_value!(InferenceRunId);
string_value!(SyncRunId);
string_value!(ChangeId);
string_value!(ExternalId);
string_value!(Scope);
string_value!(Fingerprint);
string_value!(EvidenceKey);
string_value!(FieldPath);
string_value!(RuleVersion);
string_value!(SyncCursor);

fn validate_token(value: &str, type_name: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(DomainError::invalid_value(format!(
            "{type_name} must be a lowercase token"
        )));
    }
    Ok(())
}

fn validate_namespaced(value: &str, type_name: &str) -> Result<(), DomainError> {
    let segments = value.split('.').collect::<Vec<_>>();
    if segments.len() < 2 {
        return Err(DomainError::invalid_value(format!(
            "{type_name} must contain a namespace"
        )));
    }
    for segment in segments {
        validate_token(segment, type_name)?;
    }
    Ok(())
}

macro_rules! validated_string {
    ($name:ident, $validator:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                $validator(&value, stringify!($name))?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = DomainError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

validated_string!(ConnectorType, validate_token);
validated_string!(ResourceKind, validate_namespaced);
validated_string!(RelationKind, validate_namespaced);
validated_string!(LabelKey, validate_namespaced);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaVersion(u32);

pub const DOMAIN_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

impl SchemaVersion {
    pub fn new(value: u32) -> Result<Self, DomainError> {
        if value == 0 {
            return Err(DomainError::invalid_value("SchemaVersion must be positive"));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    pub fn from_unix_millis(value: i64) -> Result<Self, DomainError> {
        if value < 0 {
            return Err(DomainError::invalid_value("Timestamp cannot be negative"));
        }
        Ok(Self(value))
    }

    pub const fn unix_millis(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Confidence(u16);

impl Confidence {
    pub fn from_basis_points(value: u16) -> Result<Self, DomainError> {
        if value > 10_000 {
            return Err(DomainError::invalid_value(
                "Confidence cannot exceed 10000 basis points",
            ));
        }
        Ok(Self(value))
    }

    pub const fn basis_points(self) -> u16 {
        self.0
    }
}
