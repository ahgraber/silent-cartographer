//! Derives the CLI's structural surface — commands, positional arguments, flags, their enumerated
//! values, and defaults — by walking the built clap [`clap::Command`] tree, so the `manifest`
//! command's answer stays in lockstep with the parser rather than a hand-written copy that drifts.
//!
//! `manifest` deliberately omits per-flag prose and response-shape vocabulary: that is `--help`'s
//! job, which the derived structure points at through [`HELP_HINT`].

use clap::{Arg, ArgAction, Command};
use serde::Serialize;

/// The command/flag structure's version. Bump this whenever the derived structure changes — a
/// command or flag added, removed, or renamed; a positional argument's requiredness changed; a
/// `ValueEnum` gaining or losing a value; a default changing.
///
/// A snapshot test (`tests/surface_manifest.rs`) pins the derived structure to a checked-in fixture
/// and fails when it changes; the documented remedy is to regenerate the fixture and bump this
/// constant in the same change, so the surface can never drift silently.
pub const SURFACE_VERSION: u32 = 9;

/// Where the prose `manifest` omits actually lives, so a caller reading the structural index knows
/// where to find it.
const HELP_HINT: &str = "c10r <command> --help";

/// A positional argument: its name, whether the invocation must supply it, its enumerated valid values
/// (for a `ValueEnum`-backed positional), and its default value when one is defined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArgManifest {
    pub name: String,
    pub required: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// A named flag: whether it takes a value, its enumerated valid values (for a `ValueEnum`-backed
/// flag), its default value, and whether it is accepted globally — before or after any subcommand —
/// rather than local to the one command it is listed under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlagManifest {
    pub name: String,
    pub takes_value: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    pub global: bool,
}

/// One command's structure: its name, one-line summary, positional arguments, and flags — its own
/// plus every inherited global flag, each tagged with [`FlagManifest::global`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandManifest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    pub args: Vec<ArgManifest>,
    pub flags: Vec<FlagManifest>,
}

/// The derived surface structure: every command's positional arguments and flags, the surface
/// version they were derived under, and a pointer at `--help` for the prose omitted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SurfaceStructure {
    pub surface_version: u32,
    pub help: String,
    pub commands: Vec<CommandManifest>,
}

fn flag_manifest(arg: &Arg, global: bool) -> FlagManifest {
    let name = arg
        .get_long()
        .expect("caller filters to flags carrying a long spelling")
        .to_string();
    let takes_value = matches!(arg.get_action(), ArgAction::Set | ArgAction::Append);
    let values = if takes_value {
        arg.get_possible_values()
            .iter()
            .map(|v| v.get_name().to_string())
            .collect()
    } else {
        Vec::new()
    };
    let default = arg
        .get_default_values()
        .first()
        .map(|v| v.to_string_lossy().to_string());
    FlagManifest {
        name,
        takes_value,
        values,
        default,
        global,
    }
}

fn arg_manifest(arg: &Arg) -> ArgManifest {
    let values = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_string())
        .collect();
    let default = arg
        .get_default_values()
        .first()
        .map(|v| v.to_string_lossy().to_string());
    ArgManifest {
        name: arg.get_id().to_string(),
        required: arg.is_required_set(),
        values,
        default,
    }
}

/// Walk `cmd`'s subcommands, deriving each one's positional arguments and flags.
///
/// Global flags (defined once on the top-level command) are appended to every subcommand's flag
/// list, tagged `global: true`, since clap accepts them with any subcommand — an agent reading one
/// command's entry sees its complete accepted vocabulary without cross-referencing a separate
/// section. Order is clap's own definition order: each subcommand's own flags first, then the
/// inherited globals in the top-level command's definition order.
fn derive_commands(cmd: &Command) -> Vec<CommandManifest> {
    let globals: Vec<FlagManifest> = cmd
        .get_arguments()
        .filter(|a| a.is_global_set() && a.get_long().is_some())
        .map(|a| flag_manifest(a, true))
        .collect();

    cmd.get_subcommands()
        .map(|sub| {
            let mut args = Vec::new();
            let mut flags = Vec::new();
            for arg in sub.get_arguments() {
                if arg.is_positional() {
                    args.push(arg_manifest(arg));
                } else if let Some(long) = arg.get_long()
                    && long != "help"
                    && long != "version"
                {
                    flags.push(flag_manifest(arg, false));
                }
            }
            flags.extend(globals.iter().cloned());
            CommandManifest {
                name: sub.get_name().to_string(),
                about: sub.get_about().map(|s| s.to_string()),
                args,
                flags,
            }
        })
        .collect()
}

/// Derive the full structural surface from the built clap command tree, paired with
/// [`SURFACE_VERSION`].
pub fn derive_structure(cmd: &Command) -> SurfaceStructure {
    SurfaceStructure {
        surface_version: SURFACE_VERSION,
        help: HELP_HINT.to_string(),
        commands: derive_commands(cmd),
    }
}

/// The `manifest` command's full answer: the derived structure alongside the current index state.
pub fn to_manifest_json(cmd: &Command, index: serde_json::Value) -> serde_json::Value {
    let mut value = serde_json::to_value(derive_structure(cmd)).expect("SurfaceStructure serializes");
    value["index"] = index;
    value
}
