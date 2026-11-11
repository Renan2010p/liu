// SPDX-License-Identifier: MPL-2.0

//! Command line of the kernel process.
//!
//! The supervisor will eventually start `liu-kernel` with descriptors it has
//! already opened; until then the kernel process takes its few parameters on
//! the command line.

use std::path::PathBuf;

use liu_abi::PAGE_SIZE;

/// Default guest physical memory size.
pub const DEFAULT_MEMORY: u64 = 64 << 20;

/// Parameters of one kernel process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config
{
    /// Kernel image to load.
    pub kernel: PathBuf,
    /// Guest physical memory size in bytes, a multiple of the page size.
    pub memory: u64,
    /// Kernel command line.
    pub cmdline: String,
}

/// Usage text for `liu-kernel`.
pub const USAGE: &str = "usage: liu-kernel --kernel IMAGE [--memory SIZE] [--cmdline TEXT]";

/// Parses a size such as `4096`, `512K`, `64M` or `2G`.
pub fn
parse_size(text: &str) -> Option<u64>
{
    let (digits, shift): (&str, u32) = match text.as_bytes().last()
    {
        Some(b'K') | Some(b'k') => (&text[..text.len() - 1], 10),
        Some(b'M') | Some(b'm') => (&text[..text.len() - 1], 20),
        Some(b'G') | Some(b'g') => (&text[..text.len() - 1], 30),
        _ => (text, 0),
    };
    let value: u64 = digits.parse::<u64>().ok()?;

    return (value.checked_mul(1u64 << shift));
}

impl Config
{
    /// Parses the arguments that follow the program name.
    pub fn
    parse<I>(args: I) -> Result<Config, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut kernel: Option<PathBuf> = None;
        let mut memory: u64 = DEFAULT_MEMORY;
        let mut cmdline: String = String::new();
        let mut iter: I::IntoIter = args.into_iter();

        while let Some(arg) = iter.next()
        {
            let value: &mut dyn FnMut(&str) -> Result<String, String> = &mut |name: &str|
                -> Result<String, String>
            {
                return (iter.next().ok_or(format!("{} needs a value", name)));
            };

            match arg.as_str()
            {
                "--kernel" =>
                {
                    kernel = Some(PathBuf::from(value("--kernel")?));
                }
                "--memory" =>
                {
                    let text: String = value("--memory")?;

                    memory = parse_size(&text).ok_or(format!("bad memory size: {}", text))?;
                }
                "--cmdline" =>
                {
                    cmdline = value("--cmdline")?;
                }
                other =>
                {
                    return (Err(format!("unknown argument: {}", other)));
                }
            }
        }

        let kernel: PathBuf = kernel.ok_or(String::from("--kernel is required"))?;

        if memory == 0 || memory % PAGE_SIZE != 0
        {
            return (Err(format!("memory size must be a non-zero multiple of {}", PAGE_SIZE)));
        }

        return (Ok(Config { kernel: kernel, memory: memory, cmdline: cmdline }));
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn
    args(list: &[&str]) -> Vec<String>
    {
        return (list.iter().map(|s: &&str| -> String { return (s.to_string()); }).collect());
    }

    #[test]
    fn
    parses_sizes() -> ()
    {
        assert_eq!(parse_size("4096"), Some(4096));
        assert_eq!(parse_size("512K"), Some(512 << 10));
        assert_eq!(parse_size("64M"), Some(64 << 20));
        assert_eq!(parse_size("2g"), Some(2 << 30));
        assert_eq!(parse_size("M"), None);
        assert_eq!(parse_size("99999999999G"), None);
    }

    #[test]
    fn
    parses_full_command_line() -> ()
    {
        let config: Config =
            Config::parse(args(&["--kernel", "k", "--memory", "16M", "--cmdline", "a b"])).unwrap();

        assert_eq!(config.kernel, PathBuf::from("k"));
        assert_eq!(config.memory, 16 << 20);
        assert_eq!(config.cmdline, "a b");
    }

    #[test]
    fn
    applies_defaults() -> ()
    {
        let config: Config = Config::parse(args(&["--kernel", "k"])).unwrap();

        assert_eq!(config.memory, DEFAULT_MEMORY);
        assert_eq!(config.cmdline, "");
    }

    #[test]
    fn
    rejects_bad_input() -> ()
    {
        assert!(Config::parse(args(&[])).is_err());
        assert!(Config::parse(args(&["--kernel"])).is_err());
        assert!(Config::parse(args(&["--kernel", "k", "--memory", "1000"])).is_err());
        assert!(Config::parse(args(&["--kernel", "k", "--bogus"])).is_err());
    }
}
