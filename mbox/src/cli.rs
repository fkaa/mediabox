use std::path::PathBuf;
use std::str::FromStr;

xflags::xflags! {
    src "./src/cli.rs"

    cmd mbox {
        repeated -v, --verbose

        cmd analyze {
            optional -i, --input input: PathBuf

            cmd codec {

            }

            cmd packets {
                optional --packets packet_filter: PacketFilter
                optional --nal nal_filter: NalFilter
            }
        }
    }
}

#[derive(Debug)]
pub struct PacketFilter {}

impl FromStr for PacketFilter {
    type Err = anyhow::Error;

    fn from_str(_val: &str) -> Result<Self, Self::Err> {
        Ok(PacketFilter {})
    }
}

#[derive(Debug)]
pub struct NalFilter {}

impl FromStr for NalFilter {
    type Err = anyhow::Error;

    fn from_str(_val: &str) -> Result<Self, Self::Err> {
        Ok(NalFilter {})
    }
}

// generated start
// The following code is generated by `xflags` macro.
// Run `env UPDATE_XFLAGS=1 cargo build` to regenerate.
#[derive(Debug)]
pub struct Mbox {
    pub verbose: u32,
    pub subcommand: MboxCmd,
}

#[derive(Debug)]
pub enum MboxCmd {
    Analyze(Analyze),
}

#[derive(Debug)]
pub struct Analyze {
    pub input: Option<PathBuf>,
    pub subcommand: AnalyzeCmd,
}

#[derive(Debug)]
pub enum AnalyzeCmd {
    Codec(Codec),
    Packets(Packets),
}

#[derive(Debug)]
pub struct Codec;

#[derive(Debug)]
pub struct Packets {
    pub packets: Option<PacketFilter>,
    pub nal: Option<NalFilter>,
}

impl Mbox {
    #[allow(dead_code)]
    pub fn from_env_or_exit() -> Self {
        Self::from_env_or_exit_()
    }

    #[allow(dead_code)]
    pub fn from_env() -> xflags::Result<Self> {
        Self::from_env_()
    }

    #[allow(dead_code)]
    pub fn from_vec(args: Vec<std::ffi::OsString>) -> xflags::Result<Self> {
        Self::from_vec_(args)
    }
}
// generated end
