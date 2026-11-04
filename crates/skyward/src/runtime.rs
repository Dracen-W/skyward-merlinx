//! The runtime the installed system's own programs ask for: the loader they
//! name, and the folders of libraries they were built against.
//!
//! A recovery carries a system of its own, built for the recovery's own
//! programs. Where the installed system is a newer Android than that, its
//! programs cannot run against the recovery's loader and libraries: the
//! loader and the lowest library are built together and agree on how a
//! program's own storage is laid out. The installed system keeps a loader
//! of its own beside its system, meant to run before the rest of its system
//! is mounted, and that is the one its programs are run with.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the installed system keeps the loader its own programs ask for. It
/// is the system's own file, not a link into a bundle: a link would lead
/// into the bundle the recovery cannot open.
const LOADER: &str = "/mnt/system/system/bin/bootstrap/linker64";

/// The folders the installed system's programs find their libraries in,
/// the ones of their own partitions first.
const LIBRARIES: [&str; 6] = [
    "/vendor/lib64",
    "/system_ext/lib64",
    "/product/lib64",
    "/odm/lib64",
    "/mnt/system/system/lib64/bootstrap",
    "/mnt/system/system/lib64",
];

/// The installed system's runtime.
#[derive(Debug, Clone)]
pub struct Runtime {
    loader: PathBuf,
    libraries: Vec<PathBuf>,
}

impl Runtime {
    /// The runtime of the installed system, where it has one of its own.
    /// One that keeps its loader in the bundle of its runtime, as older
    /// systems do, has none: its programs run against the recovery's own,
    /// as they always have.
    pub fn installed() -> Option<Self> {
        let loader = Path::new(LOADER);
        // Only a file of the installed system's own is its loader. A link
        // leads to a loader the recovery's system keeps, or into the bundle
        // the recovery cannot open, and neither is the installed one.
        let is_own_file = fs::symlink_metadata(loader).is_ok_and(|m| m.file_type().is_file());
        if !is_own_file {
            return None;
        }
        let libraries = LIBRARIES.iter().map(PathBuf::from).filter(|p| p.is_dir()).collect();
        Some(Self { loader: loader.to_path_buf(), libraries })
    }

    pub fn loader(&self) -> &Path {
        &self.loader
    }

    /// A command that runs `program` with this runtime. The loader is named
    /// as the program, and `program` as its first argument: the loader then
    /// runs it, and the program's own name stays in the command line for
    /// whoever looks for a running program by the path it was started from.
    pub fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(&self.loader);
        cmd.arg(program);
        cmd
    }

    /// The libraries a program of the installed system is given, its own
    /// first and this runtime's after.
    pub fn libraries_for(&self, own: &[String]) -> Vec<String> {
        let mut libraries: Vec<String> = own.iter().filter(|d| !is_recovery_system(d)).cloned().collect();
        libraries.extend(self.libraries.iter().map(|p| p.to_string_lossy().into_owned()));
        libraries
    }
}

/// Whether a library folder is the recovery's own system, which must not be
/// searched before the installed system's own: the loader and the lowest
/// library would then come from different builds.
fn is_recovery_system(dir: &str) -> bool {
    dir == "/system/lib64" || dir == "/system/lib"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recovery_system_is_not_searched_for_the_installed_systems_programs() {
        let runtime = Runtime { loader: PathBuf::from(LOADER), libraries: vec![PathBuf::from("/mnt/system/system/lib64")] };
        let libraries = runtime.libraries_for(&["/vendor/lib64".into(), "/system/lib64".into()]);
        assert_eq!(libraries, ["/vendor/lib64", "/mnt/system/system/lib64"]);
    }
}
