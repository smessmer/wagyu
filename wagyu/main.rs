//! # Wagyu CLI
//!
//! A command-line tool to generate cryptocurrency wallets.

use wagyu::cli::bitcoin::BitcoinCLI;
use wagyu::cli::ethereum::EthereumCLI;
use wagyu::cli::monero::MoneroCLI;
use wagyu::cli::zcash::ZcashCLI;
use wagyu::cli::{CLIError, CLI};

use clap::{App, AppSettings};

#[cfg_attr(tarpaulin, skip)]
fn main() -> Result<(), CLIError> {
    let arguments = App::new("wagyu")
        .version("v0.6.0")
        .about("Generate a wallet for Bitcoin, Ethereum, Monero, and Zcash")
        .author("Argus <team@argus.dev>")
        .settings(&[
            AppSettings::SubcommandRequiredElseHelp,
            AppSettings::DisableHelpSubcommand,
            AppSettings::DisableVersion,
        ])
        .subcommands(vec![
            BitcoinCLI::new(),
            EthereumCLI::new(),
            MoneroCLI::new(),
            ZcashCLI::new(),
        ])
        .after_help("")
        .set_term_width(100)
        .get_matches();

    match arguments.subcommand() {
        ("bitcoin", Some(arguments)) => BitcoinCLI::print(BitcoinCLI::parse(arguments)?),
        ("ethereum", Some(arguments)) => EthereumCLI::print(EthereumCLI::parse(arguments)?),
        ("monero", Some(arguments)) => MoneroCLI::print(MoneroCLI::parse(arguments)?),
        ("zcash", Some(arguments)) => ZcashCLI::print(ZcashCLI::parse(arguments)?),
        _ => unreachable!(),
    }
}
