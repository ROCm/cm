// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use clap::{
    builder::{ArgAction, PossibleValue, TypedValueParser},
    error::{ContextKind, ContextValue},
};
use std::ffi::{OsStr, OsString};
use std::str::{FromStr, ParseBoolError};

type ClapError = clap::Error;
type ClapErrorKind = clap::error::ErrorKind;

/// A clap::TypedValueParser parser for cases where there is some incomplete list of known values,
/// and as a fallback the user should be able to supply any string they want. Also supports a
/// related mode with an "inferable prefix" for things like LLVM's "check-X" targets so the user
/// can just say "X" or any unambiguous prefix of "X", or can supply any string beginnig with
/// "check-".
#[derive(Clone)]
pub struct FuzzyParser {
    known_values: Vec<&'static str>,
    inferable_prefix: Option<&'static str>,
}

impl FuzzyParser {
    pub fn new(
        known_values: impl Into<Vec<&'static str>>,
        inferable_prefix: Option<&'static str>,
    ) -> Self {
        Self {
            known_values: known_values.into(),
            inferable_prefix,
        }
    }

    fn error(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        val: impl Into<String>,
    ) -> ClapError {
        let mut err = ClapError::new(ClapErrorKind::InvalidValue).with_cmd(cmd);
        if let Some(arg) = arg {
            err.insert(
                ContextKind::InvalidArg,
                ContextValue::String(arg.to_string()),
            );
        }
        err.insert(ContextKind::InvalidValue, ContextValue::String(val.into()));
        // We mention the inferable_prefix here to make it clear that there is a "namespace" where
        // any string is legal, alongside the incomplete set of known values. We do not include
        // this in the possible_values proper as we it would confuse the autocomplete generation.
        let mut valid_values = Vec::new();
        if let Some(prefix) = self.inferable_prefix {
            valid_values.push(format!("{prefix}*"));
        }
        valid_values.extend(self.known_values.iter().copied().map(String::from));
        err.insert(ContextKind::ValidValue, ContextValue::Strings(valid_values));
        err
    }

    fn parse_ref_without_inferable_prefix(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &str,
    ) -> Result<String, ClapError> {
        let matching = self
            .known_values
            .iter()
            .filter(|s| s.eq_ignore_ascii_case(value))
            .collect::<Vec<_>>();
        match matching[..] {
            [unique] => Ok(unique.to_string()),
            _ => Ok(value.to_string()),
        }
    }

    fn parse_ref_with_inferable_prefix(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &str,
        inferable_prefix: &str,
    ) -> Result<String, ClapError> {
        if value.starts_with(inferable_prefix) {
            return Ok(value.to_string());
        }
        let matching = self
            .known_values
            .iter()
            .filter(|s| s.starts_with(value))
            .collect::<Vec<_>>();
        match matching[..] {
            [unique] => Ok(format!("{inferable_prefix}{unique}")),
            _ => Err(self.error(cmd, arg, value)),
        }
    }
}

impl TypedValueParser for FuzzyParser {
    type Value = String;

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(
            self.known_values.iter().copied().map(PossibleValue::new),
        ))
    }

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> Result<Self::Value, ClapError> {
        let value = value
            .to_str()
            .ok_or_else(|| ClapError::new(ClapErrorKind::InvalidUtf8))?;
        match self.inferable_prefix {
            None => self.parse_ref_without_inferable_prefix(cmd, arg, value),
            Some(inferable_prefix) => {
                self.parse_ref_with_inferable_prefix(cmd, arg, value, inferable_prefix)
            }
        }
    }
}

/// A newtype around a bool that implements `AsRef<OsStr>`, so it can be used
/// with `#[derive(ArgsToVec)]`.
#[derive(Clone, Copy)]
pub struct Bool(pub bool);

impl FromStr for Bool {
    type Err = ParseBoolError;
    fn from_str(s: &str) -> Result<Bool, ParseBoolError> {
        Ok(Bool(bool::from_str(s)?))
    }
}

impl AsRef<OsStr> for Bool {
    fn as_ref(&self) -> &OsStr {
        match self.0 {
            true => "true".as_ref(),
            false => "false".as_ref(),
        }
    }
}

/// Trait implemented by `#[derive(ArgsToVec)]`
pub trait ArgsToVec {
    /// Build a vector of arguments which would be interpreted by clap in such a way as to
    /// reproduce the Args struct this is called on.
    fn args_to_vec(&self) -> Vec<OsString>;
}

/// Trait to allow settable_bool to be used in `#[arg(...)]` attributes.
pub trait SettableBool {
    /// Set Arg options to make a bool-type argument fully "settable"/overridable, such that all of
    /// these set the option -b/--bool to true:
    ///
    ///   -b
    ///   --bool
    ///   --bool=true
    ///
    /// And the following sets it to false:
    ///
    ///   --bool=false
    ///
    /// This is a key part of supporting a command-line argument based config file format, as a
    /// user can set a default in their config:
    ///
    ///   --bool=true
    ///
    /// And then on the command-line choose to override with --bool=false, whereas with the default
    /// bool parsing behavior (i.e. the long form taking no value) they have no option to.
    fn settable_bool(self) -> Self;
}

impl SettableBool for clap::Arg {
    fn settable_bool(self) -> Self {
        self.value_name("BOOL")
            .num_args(0..=1)
            .require_equals(true)
            .default_missing_value("true")
    }
}

pub trait OverridingVec {
    fn overriding_vec(self) -> Self;
}

impl OverridingVec for clap::Arg {
    fn overriding_vec(self) -> Self {
        self.action(ArgAction::Set).value_delimiter(',')
    }
}
