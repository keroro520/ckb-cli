use crate::subcommands::{CliSubCommand, DAOSubCommand};
use crate::utils::arg;
use crate::utils::printer::OutputFormat;
use clap::{App, ArgMatches, SubCommand};

impl<'a> CliSubCommand for DAOSubCommand<'a> {
    fn process(
        &mut self,
        matches: &ArgMatches,
        format: OutputFormat,
        color: bool,
        debug: bool,
    ) -> Result<String, String> {
        match matches.subcommand() {
            ("deposit", Some(m)) => self.deposit(m, format, color, debug),
            ("prepare", Some(m)) => self.prepare(m, format, color, debug),
            ("withdraw", Some(m)) => self.withdraw(m, format, color, debug),
            ("query-deposited-live-cells", Some(m)) => {
                self.query_live_deposited_cells(m, format, color, debug)
            }
            ("query-prepared-live-cells", Some(m)) => {
                self.query_live_prepared_cells(m, format, color, debug)
            }
            _ => Err(matches.usage().to_owned()),
        }
    }
}

impl<'a> DAOSubCommand<'a> {
    pub fn subcommand() -> App<'static, 'static> {
        SubCommand::with_name("dao")
            .about("Deposit / prepare / withdraw / query NervosDAO balance (with local index) / key utils")
            .subcommands(vec![
                SubCommand::with_name("deposit")
                    .about("Deposit capacity into NervosDAO")
                    .arg(arg::privkey_path().required_unless(arg::from_account().b.name))
                    .arg(arg::from_account().required_unless(arg::privkey_path().b.name))
                    .arg(arg::capacity().required(true))
                    .arg(arg::tx_fee().required(true))
                    .arg(arg::with_password()),
                SubCommand::with_name("prepare")
                    .about("Prepare capacity from NervosDAO")
                    .arg(arg::privkey_path().required_unless(arg::from_account().b.name))
                    .arg(arg::from_account().required_unless(arg::privkey_path().b.name))
                    .arg(arg::capacity().required(true))
                    .arg(arg::tx_fee().required(true))
                    .arg(arg::with_password()),
                SubCommand::with_name("withdraw")
                    .about("Withdraw capacity from NervosDAO")
                    .arg(arg::privkey_path().required_unless(arg::from_account().b.name))
                    .arg(arg::from_account().required_unless(arg::privkey_path().b.name))
                    .arg(arg::capacity().required(true))
                    .arg(arg::tx_fee().required(true))
                    .arg(arg::with_password()),
                SubCommand::with_name("query-deposited-live-cells")
                    .about("Query NervosDAO deposited capacity by lock script hash or address")
                    .arg(arg::lock_hash())
                    .arg(arg::address()),
                SubCommand::with_name("query-prepared-live-cells")
                    .about("Query NervosDAO prepared capacity by lock script hash or address")
                    .arg(arg::lock_hash())
                    .arg(arg::address())
            ])
    }
}
