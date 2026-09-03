//! `Gem::Platform`: which machine a gem was built for.
//!
//! A lockfile names one row per platform a gem resolves for, and a `.gem`
//! whose platform is not `ruby` ships a prebuilt binary. zeo reads both to
//! decide which row to fetch and whether the gem carries native code.
//!
//! The parse follows `platform.rb`: split on `-`, glue a trailing libc name
//! back onto the OS (`x86_64-linux-gnu` is cpu `x86_64`, os `linux`, version
//! `gnu`), and treat a trailing all-digit part as the OS version
//! (`arm64-darwin-23`).

use std::fmt;

/// The platforms whose name carries a version RubyGems splits off.
const VERSIONED_OS: &[&str] = &[
    "darwin",
    "freebsd",
    "netbsd",
    "openbsd",
    "dragonfly",
    "solaris",
    "aix",
    "hpux",
    "mingw",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Platform {
    /// The source gem: pure Ruby, built at install time if it has extensions.
    Ruby,
    /// A built gem: `arm64-darwin`, `x86_64-linux-gnu`, `java`.
    Specific {
        /// `None` for a platform with no cpu part (`java`).
        cpu: Option<String>,
        os: String,
        /// The OS version or libc (`23`, `gnu`, `musl`), when named.
        version: Option<String>,
    },
}

impl Platform {
    /// Parse a platform string. `"ruby"` and an empty string are [`Platform::Ruby`].
    pub fn parse(text: &str) -> Platform {
        let text = text.trim();
        if text.is_empty() || text == "ruby" {
            return Platform::Ruby;
        }
        let mut parts: Vec<String> = text.split('-').map(String::from).collect();
        // `x86_64-linux-gnu` -- a trailing part that is not all digits belongs
        // to the OS, not to a version of its own.
        if parts.len() > 2 && !is_os_version(parts.last().expect("checked len")) {
            let extra = parts.pop().expect("checked len");
            let last = parts.last_mut().expect("checked len");
            last.push('-');
            last.push_str(&extra);
        }
        let (cpu, rest) = if parts.len() == 1 {
            (None, parts)
        } else {
            let cpu = parts.remove(0);
            // RubyGems folds every 32-bit x86 spelling into one name.
            let cpu = if is_i386(&cpu) {
                "x86".to_string()
            } else {
                cpu
            };
            (Some(cpu), parts)
        };
        let (os, version) = split_os(&rest);
        Platform::Specific { cpu, os, version }
    }

    /// The platform this build of the crate targets, in RubyGems' spelling.
    pub fn host() -> Platform {
        Platform::from_target_triple(HOST_TRIPLE)
    }

    /// A Rust target triple as RubyGems would name it:
    /// `aarch64-apple-darwin` -> `arm64-darwin`.
    ///
    /// A triple this table does not know is parsed as written, so a new
    /// target names something plausible instead of nothing.
    pub fn from_target_triple(triple: &str) -> Platform {
        let name = match triple {
            "aarch64-apple-darwin" => "arm64-darwin",
            "x86_64-apple-darwin" => "x86_64-darwin",
            "x86_64-unknown-linux-gnu" => "x86_64-linux",
            "aarch64-unknown-linux-gnu" => "aarch64-linux",
            "x86_64-unknown-linux-musl" => "x86_64-linux-musl",
            "aarch64-unknown-linux-musl" => "aarch64-linux-musl",
            other => other,
        };
        Platform::parse(name)
    }

    pub fn is_ruby(&self) -> bool {
        *self == Platform::Ruby
    }

    /// Whether a gem built for `self` runs on `host`.
    ///
    /// `Gem::Platform#===`, with its three concessions: `universal` matches
    /// any cpu, a gem with no OS version runs on every version of that OS, and
    /// a linux gem with no libc named runs against glibc.
    pub fn matches(&self, host: &Platform) -> bool {
        match (self, host) {
            (Platform::Ruby, _) => true,
            (Platform::Specific { .. }, Platform::Ruby) => false,
            (
                Platform::Specific {
                    cpu: a_cpu,
                    os: a_os,
                    version: a_ver,
                },
                Platform::Specific {
                    cpu: b_cpu,
                    os: b_os,
                    version: b_ver,
                },
            ) => {
                let cpu_ok = matches!(a_cpu.as_deref(), None | Some("universal"))
                    || matches!(b_cpu.as_deref(), None | Some("universal"))
                    || a_cpu == b_cpu
                    // `arm` is the family, `armv7l` a member of it.
                    || (a_cpu.as_deref() == Some("arm")
                        && b_cpu.as_deref().is_some_and(|c| c.starts_with("arm")));
                if !cpu_ok || a_os != b_os {
                    return false;
                }
                // glibc is linux's unnamed default, so a gem built for
                // `linux` with no libc named runs on a `linux-gnu` host.
                if a_os == "linux"
                    && b_ver
                        .as_deref()
                        .is_some_and(|b| b == format!("gnu{}", a_ver.as_deref().unwrap_or("")))
                {
                    return true;
                }
                match (a_ver.as_deref(), b_ver.as_deref()) {
                    (None, _) | (_, None) => true,
                    (Some(a), Some(b)) => a == b,
                }
            }
        }
    }
}

/// A `.` separated run of digits -- an OS version, not a libc name.
fn is_os_version(part: &str) -> bool {
    !part.is_empty()
        && part.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && part.bytes().any(|b| b.is_ascii_digit())
}

fn is_i386(cpu: &str) -> bool {
    let b = cpu.as_bytes();
    b.len() == 4 && b[0] == b'i' && b[1].is_ascii_digit() && &b[2..] == b"86"
}

/// The OS and its version, from whatever is left after the cpu. Handles both
/// spellings: a separate part (`darwin-23`) and a glued one (`darwin23`).
fn split_os(parts: &[String]) -> (String, Option<String>) {
    match parts {
        [] => (String::new(), None),
        // One part, either glued (`darwin23`) or already re-joined by the
        // caller (`linux-gnu`).
        [one] => match one.split_once('-') {
            Some((os, version)) => (os.to_string(), Some(version.to_string())),
            None => split_glued_os(one),
        },
        [os, rest @ ..] => (os.clone(), Some(rest.join("-"))),
    }
}

/// `darwin23` -> (`darwin`, `23`). Only for the names RubyGems versions this
/// way; `mingw32` and `linux` keep their digits.
fn split_glued_os(text: &str) -> (String, Option<String>) {
    let split = text
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(text.len());
    let (name, digits) = text.split_at(split);
    if digits.is_empty() || !VERSIONED_OS.contains(&name) {
        return (text.to_string(), None);
    }
    (name.to_string(), Some(digits.to_string()))
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Platform::Ruby => f.write_str("ruby"),
            Platform::Specific { cpu, os, version } => {
                if let Some(cpu) = cpu {
                    write!(f, "{cpu}-")?;
                }
                f.write_str(os)?;
                match version {
                    Some(v) => write!(f, "-{v}"),
                    None => Ok(()),
                }
            }
        }
    }
}

/// The triple this crate is compiled for. Read from the standard consts so
/// the crate needs no build script.
const HOST_TRIPLE: &str = host_triple();

const fn host_triple() -> &'static str {
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    return "aarch64-apple-darwin";
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    return "x86_64-apple-darwin";
    #[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"))]
    return "aarch64-unknown-linux-gnu";
    #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
    return "x86_64-unknown-linux-gnu";
    #[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "musl"))]
    return "aarch64-unknown-linux-musl";
    #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "musl"))]
    return "x86_64-unknown-linux-musl";
    // Every target zeo ships for is above. Anywhere else the crate still
    // builds, and `Platform::host` says what it can.
    #[cfg(not(any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "macos"),
        all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"),
        all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
        all(target_arch = "aarch64", target_os = "linux", target_env = "musl"),
        all(target_arch = "x86_64", target_os = "linux", target_env = "musl"),
    )))]
    return "unknown-unknown";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Platform {
        Platform::parse(s)
    }

    #[test]
    fn ruby_is_the_source_platform() {
        assert!(p("ruby").is_ruby());
        assert!(p("").is_ruby());
        assert_eq!(p("ruby").to_string(), "ruby");
    }

    #[test]
    fn the_parse_round_trips_every_platform_a_lockfile_names() {
        for name in [
            "ruby",
            "java",
            "arm64-darwin",
            "arm64-darwin-23",
            "x86_64-darwin",
            "x86_64-linux",
            "x86_64-linux-gnu",
            "x86_64-linux-musl",
            "aarch64-linux",
            "aarch64-linux-gnu",
            "aarch64-linux-musl",
            "arm-linux-gnu",
            "arm-linux-musl",
            "x86-linux-gnu",
            "x86-linux-musl",
            "x64-mingw-ucrt",
            "universal-darwin",
        ] {
            assert_eq!(p(name).to_string(), name, "{name}");
        }
    }

    #[test]
    fn a_libc_tail_belongs_to_the_os_not_to_a_third_part() {
        let Platform::Specific { cpu, os, version } = p("x86_64-linux-gnu") else {
            panic!("expected a specific platform");
        };
        assert_eq!(cpu.as_deref(), Some("x86_64"));
        assert_eq!(os, "linux");
        assert_eq!(version.as_deref(), Some("gnu"));
    }

    #[test]
    fn a_trailing_number_is_an_os_version() {
        let Platform::Specific { os, version, .. } = p("arm64-darwin-23") else {
            panic!("expected a specific platform");
        };
        assert_eq!(os, "darwin");
        assert_eq!(version.as_deref(), Some("23"));
        // The glued spelling reads the same way.
        assert_eq!(p("arm64-darwin23"), p("arm64-darwin-23"));
    }

    #[test]
    fn every_32_bit_x86_spelling_folds_into_one_cpu() {
        assert_eq!(p("i686-linux"), p("x86-linux"));
        assert_eq!(p("i386-linux"), p("x86-linux"));
    }

    #[test]
    fn a_gem_without_an_os_version_runs_on_every_version_of_that_os() {
        assert!(p("arm64-darwin").matches(&p("arm64-darwin-23")));
        assert!(p("arm64-darwin-23").matches(&p("arm64-darwin")));
        assert!(!p("arm64-darwin-22").matches(&p("arm64-darwin-23")));
        assert!(!p("arm64-darwin").matches(&p("x86_64-darwin")));
        assert!(!p("arm64-darwin").matches(&p("arm64-linux")));
    }

    #[test]
    fn linux_without_a_libc_means_glibc() {
        assert!(p("x86_64-linux").matches(&p("x86_64-linux-gnu")));
        assert!(p("x86_64-linux-gnu").matches(&p("x86_64-linux-gnu")));
        assert!(!p("x86_64-linux-musl").matches(&p("x86_64-linux-gnu")));
        assert!(!p("x86_64-linux-gnu").matches(&p("x86_64-linux-musl")));
        assert!(p("x86_64-linux-gnu").matches(&p("x86_64-linux")));
    }

    #[test]
    fn universal_matches_any_cpu_and_ruby_matches_everything() {
        assert!(p("universal-darwin").matches(&p("arm64-darwin")));
        assert!(p("ruby").matches(&p("arm64-darwin")));
        assert!(!p("arm64-darwin").matches(&Platform::Ruby));
    }

    #[test]
    fn a_rust_triple_becomes_the_rubygems_name() {
        assert_eq!(
            Platform::from_target_triple("aarch64-apple-darwin").to_string(),
            "arm64-darwin"
        );
        assert_eq!(
            Platform::from_target_triple("x86_64-unknown-linux-gnu").to_string(),
            "x86_64-linux"
        );
        // The host is one of the table's entries on every machine zeo builds on.
        assert!(!Platform::host().is_ruby());
    }
}
