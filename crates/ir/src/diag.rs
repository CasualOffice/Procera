//! Compiler diagnostics: stable codes, severities, JSON-pointer locations (docs/11 §11).

use serde::{Serialize, Serializer};
use std::fmt;

/// How serious a diagnostic is. Any `Error` blocks the compile output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

macro_rules! codes {
    ($( $variant:ident = $code:literal, $sev:ident, $title:literal; )*) => {
        /// A stable diagnostic code. Codes are never renumbered or reused (docs/24 §1.7).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Code { $( $variant, )* }

        impl Code {
            /// Every code, in table order.
            pub const ALL: &'static [Code] = &[ $( Code::$variant, )* ];

            /// The wire form, e.g. `IR-E001`.
            pub fn as_str(self) -> &'static str {
                match self { $( Code::$variant => $code, )* }
            }

            /// The fixed severity of this code.
            pub fn severity(self) -> Severity {
                match self { $( Code::$variant => Severity::$sev, )* }
            }

            /// One-line description of the check, as listed in docs/11 "As built (BPMS-080)".
            pub fn title(self) -> &'static str {
                match self { $( Code::$variant => $title, )* }
            }
        }
    };
}

codes! {
    E001 = "IR-E001", Error, "Exactly one `start` node";
    E002 = "IR-E002", Error, "Duplicate node id";
    E003 = "IR-E003", Error, "Duplicate edge id";
    E004 = "IR-E004", Error, "Edge endpoint does not exist";
    E005 = "IR-E005", Error, "Outcome not handled (no edge and no `unhandled: fail`)";
    E006 = "IR-E006", Error, "Edge uses an outcome the node cannot produce";
    E007 = "IR-E007", Error, "Decision without a `default` edge";
    E008 = "IR-E008", Error, "Split/join pairing, or a branch escaping its join or crossing another branch";
    E009 = "IR-E009", Error, "Node unreachable from `start`";
    E010 = "IR-E010", Error, "No `end` reachable from `start`";
    E011 = "IR-E011", Error, "Loop without a human/wait node or `maxIterations`";
    E012 = "IR-E012", Error, "Size/complexity limit exceeded (500 nodes, 50 branches, nesting 10)";
    E013 = "IR-E013", Error, "Edge into `start` or out of `end`";
    E014 = "IR-E014", Error, "`excludeActorsOf` references an unknown node";
    E015 = "IR-E015", Error, "Boundary target does not exist";
    E016 = "IR-E016", Error, "Duplicate variable name";
    E017 = "IR-E017", Error, "Outcome handled by more than one edge";
    E018 = "IR-E018", Error, "Field required by the node kind is missing";
    E019 = "IR-E019", Error, "Expression is malformed or uses an unknown language";
    E020 = "IR-E020", Error, "Expression or assignment references an undeclared variable";
    E021 = "IR-E021", Error, "`restricted` variable used in a notification or event-facing field";
    E022 = "IR-E022", Error, "Node references a `refs` key that is not declared";
    E023 = "IR-E023", Error, "Reference does not resolve to a published version the author may use";
    E024 = "IR-E024", Error, "Connector not enabled for the workspace or scopes not granted";
    E025 = "IR-E025", Error, "Assignment group/role/position does not exist";
    E026 = "IR-E026", Error, "Separation of duties cannot be satisfied";
    E027 = "IR-E027", Error, "Business-time duration without a calendar";
    E028 = "IR-E028", Error, "Timer misconfigured (`at` xor `after`; timeout boundary needs `after`)";
    E029 = "IR-E029", Error, "Absolute instant without a UTC offset";
    E030 = "IR-E030", Error, "Escalation invalid (missing target/outcome, or auto-outcome in a controlled process)";
    E031 = "IR-E031", Error, "Boundary on a node kind that cannot have one";
    E032 = "IR-E032", Error, "SLA references an unknown node";
    E033 = "IR-E033", Error, "Duplicate SLA id";
    E034 = "IR-E034", Error, "Duplicate branch name";
    E100 = "IR-E100", Error, "Schema violation (shape, unknown field, pattern)";
    W001 = "IR-W001", Warning, "`readers` on a variable that is not `restricted` has no effect";
    W002 = "IR-W002", Warning, "`excludeActorsOf` references a node that has no human actors";
    I001 = "IR-I001", Info, "Environment checks (refs, connectors, directory) not performed";
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for Code {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// One compiler finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    /// RFC 6901 pointer into the *source* document (`""` is the whole document).
    pub pointer: String,
    /// The node the finding is about, when there is one (the designer links to it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub message: String,
}

impl Diagnostic {
    pub fn new(code: Code, pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code, severity: code.severity(), pointer: pointer.into(), node: None, message: message.into() }
    }

    pub fn at_node(mut self, node: &str) -> Self {
        self.node = Some(node.to_owned());
        self
    }
}

/// Escapes one reference token (RFC 6901 §3: `~` → `~0`, `/` → `~1`).
pub fn escape_pointer(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}
