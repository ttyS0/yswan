use clap::{Args, Parser, Subcommand};
use std::path::{PathBuf};

/// A layer-3 toy VPN with SSL supported.
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
pub struct Cli {
    /// Mode: "client" or "server"
    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Args)]
pub struct TlsOptions {
    /// The path to the certificate(s) of CA chains
    #[clap(long)]
    pub cacert: PathBuf,

    /// The path to X.509 certificates of this instance
    #[clap(long)]
    pub cert: PathBuf,

    /// The path to X.509 private keys of this instance
    #[clap(long)]
    pub key: PathBuf,
}

#[derive(Debug, Args)]
pub struct TunOptions {
    /// The name of the TUN device
    #[clap(long, default_value = "yswan")]
    pub tun_name: String,

    /// The "inet" address of the TUN device
    #[clap(long)]
    // pub tun_inet: Option<String>,
    pub tun_inet: String,

    /// MTU of the TUN device
    #[clap(long, default_value_t = 1400)]
    pub tun_mtu: u16,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run yswan server
    Server {
        /// The port that the server will use
        #[clap(long, default_value_t = 4508)]
        port: u16,

        #[clap(flatten)]
        tls_options: TlsOptions,

        #[clap(flatten)]
        tun_options: TunOptions,
    },
    /// Run yswan client
    Client {
        /// Server address
        #[clap(long)]
        connect: String,

        /// Username for authentication
        #[clap(long)]
        username: Option<String>,

        /// Password for authentication
        #[clap(long)]
        password: Option<String>,

        /// Install routes
        #[clap(long)]
        routes: Option<Vec<String>>,

        #[clap(flatten)]
        tls_options: TlsOptions,

        #[clap(flatten)]
        tun_options: TunOptions,
    },
}
