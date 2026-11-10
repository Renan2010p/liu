// SPDX-License-Identifier: MPL-2.0

//! Host description produced by `probe()`.

use std::fmt;

/// Host operating system family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HostOs
{
    /// FreeBSD, including Renux (see [`RenuxExtensions`]).
    FreeBsd,
    Linux,
    NetBsd,
    DragonFly,
    MacOs,
    OpenBsd,
    Illumos,
    Other,
}

impl HostOs
{
    /// Classifies a `uname(3)` system name.
    pub fn
    from_sysname(sysname: &str) -> HostOs
    {
        let os: HostOs = match sysname
        {
            "FreeBSD" => HostOs::FreeBsd,
            "Linux" => HostOs::Linux,
            "NetBSD" => HostOs::NetBsd,
            "DragonFly" => HostOs::DragonFly,
            "Darwin" => HostOs::MacOs,
            "OpenBSD" => HostOs::OpenBsd,
            "SunOS" => HostOs::Illumos,
            _ => HostOs::Other,
        };

        return (os);
    }
}

impl fmt::Display for HostOs
{
    fn
    fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        let name: &str = match self
        {
            HostOs::FreeBsd => "FreeBSD",
            HostOs::Linux => "Linux",
            HostOs::NetBsd => "NetBSD",
            HostOs::DragonFly => "DragonFly BSD",
            HostOs::MacOs => "macOS",
            HostOs::OpenBsd => "OpenBSD",
            HostOs::Illumos => "illumos",
            HostOs::Other => "unknown Unix",
        };

        return (f.write_str(name));
    }
}

/// Mechanisms for trapping guest system calls (architecture.md §7.3).
///
/// A kind listed in [`Capabilities::trap_backends`] means the host provides
/// the underlying mechanism; the backend itself may not be implemented yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrapBackendKind
{
    /// Renux `PT_TO_SCE` + `PT_SET_SC_RET`: one stop per call.
    RenuxSetScRet,
    /// Stock FreeBSD/DragonFly: rewrite the call at entry, fix the result at
    /// exit. Two stops per call.
    FreeBsdRewrite,
    /// Linux `PTRACE_SYSEMU`.
    LinuxSysemu,
    /// NetBSD `PT_SYSCALLEMU`.
    NetBsdSyscallEmu,
    /// Linux KVM, guest user code only.
    Kvm,
    /// macOS Hypervisor.framework, guest user code only.
    Hvf,
}

impl TrapBackendKind
{
    /// Stable name used in configuration files and logs.
    pub fn
    name(self) -> &'static str
    {
        let name: &str = match self
        {
            TrapBackendKind::RenuxSetScRet => "renux-setscret",
            TrapBackendKind::FreeBsdRewrite => "freebsd-rewrite",
            TrapBackendKind::LinuxSysemu => "linux-sysemu",
            TrapBackendKind::NetBsdSyscallEmu => "netbsd-syscallemu",
            TrapBackendKind::Kvm => "kvm",
            TrapBackendKind::Hvf => "hvf",
        };

        return (name);
    }
}

/// Sandboxing mechanism `enter_sandbox()` uses on this host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SandboxKind
{
    /// FreeBSD/Renux capability mode.
    Capsicum,
    /// No sandbox implementation on this host yet.
    None,
}

/// Renux kernel extensions found on a FreeBSD-family host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenuxExtensions
{
    /// `PT_SET_SC_RET` (ptrace request 45) is available.
    pub set_sc_ret: bool,
}

impl RenuxExtensions
{
    /// Returns whether any Renux extension is present.
    pub fn
    any(&self) -> bool
    {
        return (self.set_sc_ret);
    }
}

/// Result of probing the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities
{
    pub os: HostOs,
    /// `uname(3)` release string.
    pub release: String,
    /// `uname(3)` machine string.
    pub machine: String,
    pub renux: RenuxExtensions,
    /// Available trap mechanisms, fastest first.
    pub trap_backends: Vec<TrapBackendKind>,
    pub sandbox: SandboxKind,
}

impl Capabilities
{
    /// Returns whether the host is Renux (FreeBSD with Renux extensions).
    pub fn
    is_renux(&self) -> bool
    {
        return (self.os == HostOs::FreeBsd && self.renux.any());
    }

    /// The fastest trap mechanism, if the host has any.
    pub fn
    best_trap_backend(&self) -> Option<TrapBackendKind>
    {
        return (self.trap_backends.first().copied());
    }

    /// Human-readable host name, e.g. "Renux (FreeBSD 15.1-RELEASE-p6)".
    pub fn
    host_name(&self) -> String
    {
        if self.is_renux()
        {
            return (format!("Renux ({} {})", self.os, self.release));
        }

        return (format!("{} {}", self.os, self.release));
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn
    sample(os: HostOs, renux: bool) -> Capabilities
    {
        let caps: Capabilities = Capabilities
        {
            os: os,
            release: String::from("15.1-RELEASE"),
            machine: String::from("amd64"),
            renux: RenuxExtensions { set_sc_ret: renux },
            trap_backends: vec![TrapBackendKind::RenuxSetScRet, TrapBackendKind::FreeBsdRewrite],
            sandbox: SandboxKind::Capsicum,
        };

        return (caps);
    }

    #[test]
    fn
    classifies_sysnames() -> ()
    {
        assert_eq!(HostOs::from_sysname("FreeBSD"), HostOs::FreeBsd);
        assert_eq!(HostOs::from_sysname("Darwin"), HostOs::MacOs);
        assert_eq!(HostOs::from_sysname("Haiku"), HostOs::Other);
    }

    #[test]
    fn
    renux_requires_freebsd_and_extensions() -> ()
    {
        assert!(sample(HostOs::FreeBsd, true).is_renux());
        assert!(!sample(HostOs::FreeBsd, false).is_renux());
        assert!(!sample(HostOs::Linux, true).is_renux());
    }

    #[test]
    fn
    names_renux_hosts() -> ()
    {
        assert_eq!(sample(HostOs::FreeBsd, true).host_name(), "Renux (FreeBSD 15.1-RELEASE)");
        assert_eq!(sample(HostOs::FreeBsd, false).host_name(), "FreeBSD 15.1-RELEASE");
    }

    #[test]
    fn
    best_backend_is_first() -> ()
    {
        let caps: Capabilities = sample(HostOs::FreeBsd, true);

        assert_eq!(caps.best_trap_backend(), Some(TrapBackendKind::RenuxSetScRet));
    }
}
