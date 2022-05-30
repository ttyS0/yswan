// 
// Copyright (c) 2022 sigeryang
// 
// @file      main.rs
// @author    Siger Yang (siger.yang@outlook.com)
// @date      May 17, 2022
// 

mod config;
mod yswan;
use std::io::{self, BufRead, Write};
use clap::Parser;

#[tokio::main]
async fn main() -> io::Result<()> {
    let cli = config::Cli::parse();

    match &cli.command {
        config::Commands::Server { port, tls_options, tun_options } => {
            yswan::Server::new(*port, tls_options, tun_options).unwrap().start().await?;
            Ok(())
        },
        config::Commands::Client { connect, username, password, tls_options, tun_options, routes } => {
            let mut new_username = String::new();
            let mut new_password = String::new();
            if let None = username {
                print!("Username: ");
                let _ = io::stdout().flush();
                let mut line = String::new();
                let stdin = io::stdin();
                stdin.lock().read_line(&mut line).unwrap();
                new_username = line.strip_suffix("\r\n")
                .or(line.strip_suffix("\n"))
                .unwrap_or(line.as_str()).to_string();
            }
            if let None = password {
                new_password = rpassword::prompt_password("Your password: ").expect("Invalid input.").to_string();
            }
            yswan::Client::new(connect, username.as_ref().unwrap_or(&new_username), password.as_ref().unwrap_or(&new_password), tls_options, tun_options).unwrap().start().await?;
            Ok(())
        },
    }
}
