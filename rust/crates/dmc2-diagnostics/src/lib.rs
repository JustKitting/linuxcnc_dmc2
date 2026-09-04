#![no_std]

use core::fmt;
use core::fmt::Write;

mod recovery;

pub use recovery::{
    RecoverableDiagnostic, RecoveryClass, RecoveryClassified, RecoveryContract, RecoveryDisplay,
    RecoveryOperation, RecoveryTransition, RECOVERY_CONTRACTS,
};

/// Complete operator-facing meaning of one stable numeric diagnostic code.
///
/// A raw value is transport compatibility only. Every owning subsystem must
/// provide the remaining fields, so callers never need a separate code table
/// to understand a reported failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticMetadata {
    wire_code: i64,
    name: &'static str,
    hal_slug: &'static str,
    summary: &'static str,
    action: &'static str,
}

impl DiagnosticMetadata {
    /// Construct one catalog entry. Literal catalogs are validated at compile
    /// time by `diagnostic_catalog!`; generic consumers must call `complete`
    /// and surface an invalid external implementation as a typed interface
    /// diagnostic rather than panicking.
    pub const fn new(
        wire_code: i64,
        name: &'static str,
        hal_slug: &'static str,
        summary: &'static str,
        action: &'static str,
    ) -> Self {
        Self {
            wire_code,
            name,
            hal_slug,
            summary,
            action,
        }
    }

    pub const fn complete(self) -> bool {
        valid_symbolic_identity(self.name)
            && valid_hal_slug(self.hal_slug)
            && !self.summary.is_empty()
            && !self.action.is_empty()
    }

    pub const fn wire_code(self) -> i64 {
        self.wire_code
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn hal_slug(self) -> &'static str {
        self.hal_slug
    }

    pub const fn summary(self) -> &'static str {
        self.summary
    }

    pub const fn action(self) -> &'static str {
        self.action
    }
}

/// Stable identities are deliberately distinct from vendor display strings.
/// They are safe in journals, UI matching, and machine-readable logs without
/// any per-domain parsing rule.
pub const fn valid_symbolic_identity(identity: &str) -> bool {
    let bytes = identity.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_uppercase() {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_') {
            return false;
        }
        index += 1;
    }
    true
}

/// Diagnostic domains use one lossless transport grammar. Converting this
/// form to ASCII uppercase always yields the domain portion of a valid
/// `UNKNOWN_<DOMAIN>(raw=<value>)` identity.
pub const fn valid_diagnostic_domain(domain: &str) -> bool {
    let bytes = domain.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
            return false;
        }
        index += 1;
    }
    true
}

/// HAL slugs have one portable representation across all DMC2 components.
pub const fn valid_hal_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    if bytes.is_empty()
        || !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        || bytes[bytes.len() - 1] == b'-'
    {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
            return false;
        }
        if byte == b'-' && bytes[index - 1] == b'-' {
            return false;
        }
        index += 1;
    }
    true
}

/// Required contract for every numeric fault/error domain owned by DMC2.
pub trait SelfDescribingDiagnostic: Copy {
    fn metadata(self) -> DiagnosticMetadata;
}

/// Define a stable numeric diagnostic as one table. The table is the single
/// source for its Rust enum, wire conversion, symbolic name, HAL slug, cause,
/// operator action, and iterable catalog.
#[macro_export]
macro_rules! diagnostic_catalog {
    (
        $visibility:vis enum $name:ident {
            $(
                $variant:ident = $code:expr,
                $identity:literal,
                $slug:literal,
                $summary:literal,
                $action:literal;
            )+
        }
    ) => {
        $crate::diagnostic_catalog! {
            $visibility enum $name: i32 {
                $($variant = $code => ($identity, $slug, $summary, $action)),+
            }
        }
    };
    (
        $visibility:vis enum $name:ident : $repr:ty {
            $(
                $variant:ident = $code:expr =>
                    ($identity:literal, $slug:literal, $summary:literal, $action:literal)
            ),+ $(,)?
        }
    ) => {
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        #[repr($repr)]
        $visibility enum $name {
            $($variant = $code,)+
        }

        const _: () = {
            $(
                assert!($crate::valid_symbolic_identity($identity));
                assert!($crate::valid_hal_slug($slug));
                assert!(!$summary.is_empty());
                assert!(!$action.is_empty());
            )+
        };

        impl $name {
            pub const COUNT: usize = $crate::diagnostic_catalog!(@count $($variant),+);
            pub const ALL: [Self; Self::COUNT] = [$(Self::$variant,)+];

            pub const fn wire_code(self) -> $repr {
                self as $repr
            }

            pub const fn from_wire_code(code: $repr) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $identity,)+ }
            }

            pub const fn hal_slug(self) -> &'static str {
                match self { $(Self::$variant => $slug,)+ }
            }

            pub const fn summary(self) -> &'static str {
                match self { $(Self::$variant => $summary,)+ }
            }

            pub const fn action(self) -> &'static str {
                match self { $(Self::$variant => $action,)+ }
            }
        }

        impl $crate::SelfDescribingDiagnostic for $name {
            fn metadata(self) -> $crate::DiagnosticMetadata {
                $crate::DiagnosticMetadata::new(
                    self.wire_code() as i64,
                    self.name(),
                    self.hal_slug(),
                    self.summary(),
                    self.action(),
                )
            }
        }
    };
    (@count $($variant:ident),+) => {
        <[()]>::len(&[$($crate::diagnostic_catalog!(@unit $variant)),+])
    };
    (@unit $variant:ident) => { () };
}

/// Consistent operator-facing rendering used by logs and command output.
///
/// A numeric diagnostic cannot cross this boundary unless its complete
/// recovery state machine is also defined.
pub struct DiagnosticDisplay<T: RecoverableDiagnostic>(pub T);

impl<T: RecoverableDiagnostic> fmt::Display for DiagnosticDisplay<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let metadata = self.0.metadata();
        let recovery = self.0.recovery_class();
        if !metadata.complete() {
            let fallback = RecoveryClass::RelaunchApplication;
            write!(
                formatter,
                "DIAGNOSTIC_METADATA_INVALID: code={} name={:?} slug={:?}; cause: a diagnostic provider returned incomplete metadata; action: correct and relaunch the matched application; recovery-class={}; recovery-transition={}; clear-condition={:?}; ui-path=",
                metadata.wire_code(),
                metadata.name(),
                metadata.hal_slug(),
                fallback.name(),
                fallback.transition().name(),
                fallback.clear_transition(),
            )?;
            for (index, operation) in fallback.ui_operations().iter().enumerate() {
                if index != 0 {
                    formatter.write_str(" -> ")?;
                }
                formatter.write_str(operation.id())?;
            }
            return Ok(());
        }
        write!(
            formatter,
            "{} (code={}): {}; action: {}; recovery-class={}; recovery-transition={}; clear-condition={:?}; ui-path=",
            metadata.name(),
            metadata.wire_code(),
            metadata.summary(),
            metadata.action(),
            recovery.name(),
            recovery.transition().name(),
            recovery.clear_transition(),
        )?;
        for (index, operation) in recovery.ui_operations().iter().enumerate() {
            if index != 0 {
                formatter.write_str(" -> ")?;
            }
            formatter.write_str(operation.id())?;
        }
        Ok(())
    }
}

/// Explicit representation for source-owned values absent from a verified
/// catalog. Unknown codes retain their domain and raw value and are never
/// assigned a guessed name.
pub struct UnknownDiagnostic<'a> {
    domain: &'a str,
    raw: i64,
}

impl<'a> UnknownDiagnostic<'a> {
    /// Unknown values are allowed only with a valid source domain and their
    /// exact raw value. The identity is generated here rather than supplied
    /// by a caller, so it cannot be mislabeled.
    pub const fn new(domain: &'a str, raw: i64) -> Self {
        Self { domain, raw }
    }
}

impl fmt::Display for UnknownDiagnostic<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !valid_diagnostic_domain(self.domain) {
            return write!(
                formatter,
                "UNKNOWN_DIAGNOSTIC_DOMAIN_INVALID(raw={}): source domain {:?} violates the diagnostic grammar; action: correct and relaunch the matched application",
                self.raw, self.domain
            );
        }
        formatter.write_str("UNKNOWN_")?;
        for byte in self.domain.bytes() {
            formatter.write_char(char::from(byte).to_ascii_uppercase())?;
        }
        write!(
            formatter,
            "(raw={}): value is absent from the verified source catalog; action: retain the raw domain/value and verify the exact producer version",
            self.raw
        )
    }
}
