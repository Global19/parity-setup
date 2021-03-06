extern crate clap;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

extern crate rand;

use std::collections::HashMap;
use std::fs::File;

use clap::{Arg, App};
use serde::Serialize;
use rand::{Rng, SeedableRng};

mod generator;

static JSONRPC_VERSION: &str = "2.0";
static METHOD_NAME: &str = "personal_sendTransaction";

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Wrapper<P: Serialize> {
    jsonrpc: &'static str,
    method: &'static str,
    params: P,
    id: RpcId,
}

type RpcId = usize;
type PersonalSendTransaction = Wrapper<PersonalSendTransactionParams>;

impl PersonalSendTransaction {
    fn new(params: PersonalSendTransactionParams, id: RpcId) -> Self {
        Wrapper {
            jsonrpc: JSONRPC_VERSION,
            method: METHOD_NAME,
            params, id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Password(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(String);

#[derive(Debug, Clone, PartialEq, Serialize)]
struct PersonalSendTransactionParams(Transaction, Password);

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Transaction {
    from: AccountId,
    to: AccountId,
    value: String,
}

#[derive(Debug)]
pub struct Account {
    id: AccountId,
    balance: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct AccountConfig {
    id: AccountId,
    balance: String,
    password: Password,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigFile {
    generator: Option<String>,
    count: Option<usize>, // TODO: match this with the config flag name
    #[serde(rename = "filter-from")]
    filter_from: Option<String>,
    #[serde(rename = "chunk-size")]
    chunk_size: Option<usize>,
    seed: Option<usize>,
    accounts: Vec<AccountConfig>,
}

#[derive(Debug)]
struct Config {
    generator: Option<String>,
    count: Option<usize>,
    filter_from: Option<String>,
    chunk_size: Option<usize>,
    seed: Option<usize>,
    accounts: Vec<Account>,
    passwords: HashMap<AccountId, Password>,
}

fn parse_config_file(config_file: &str) -> Config {
    let config_file = File::open(config_file).expect("Config file not found");
    let config: ConfigFile =
        serde_json::from_reader(config_file).expect("Unable to parse config file");

    let generator = config.generator;
    let count = config.count;
    let filter_from = config.filter_from;
    let chunk_size = config.chunk_size;
    let seed = config.seed;

    let passwords =
        config.accounts.iter()
        .map(|conf| (conf.id.clone(), conf.password.clone()))
        .collect();

    let accounts =
        config.accounts.into_iter()
        .map(|conf| {
            Account {
                id: conf.id,
                balance: conf.balance.parse().expect("Unable to parse balance"),
            }
        })
        .collect();

    Config { generator, count, filter_from, chunk_size, seed, accounts, passwords }
}

fn main() {
    let matches = App::new("RPC generator")
        .arg(Arg::with_name("config")
             .long("config")
             .value_name("FILE.json")
             .takes_value(true))
        .arg(Arg::with_name("output")
             .long("output")
             .short("o")
             .value_name("OUTPUT")
             .default_value("rpc.json")
             .takes_value(true))
        .arg(Arg::with_name("generator")
             .long("generator")
             .short("g")
             .value_name("GENERATOR")
             .takes_value(true))
        .arg(Arg::with_name("transactions")
             .long("transactions")
             .value_name("N")
             .takes_value(true))
        .arg(Arg::with_name("filter-from")
             .long("filter-from")
             .value_name("ADDRESS")
             .takes_value(true))
        .arg(Arg::with_name("chunk-size")
             .long("chunk-size")
             .value_name("N")
             .takes_value(true))
        .arg(Arg::with_name("seed")
             .long("seed")
             .value_name("N")
             .takes_value(true))
        .get_matches();

    let config_file = matches.value_of("config").expect("Must provide config file");
    let output_file = matches.value_of("output").expect("Must provide output file");

    let mut config = parse_config_file(&config_file);

    let generator_arg = matches.value_of("generator").map(Into::into);
    let count_arg = matches.value_of("transactions")
        .map(|v| v.parse().expect("transactions must be a number"));
    let seed_arg =
        matches.value_of("seed")
        .map(|s| s.parse().expect("Unable to parse seed"));
    let filter_arg = matches.value_of("filter-from").map(Into::into);
    let chunks_arg =
        matches.value_of("chunk-size")
        .map(|s| s.parse().expect("Unable to parse chunk size"));

    config.generator = generator_arg.or(config.generator);
    config.count = count_arg.or(config.count);
    config.seed = seed_arg.or(config.seed);
    config.filter_from = filter_arg.or(config.filter_from);
    config.chunk_size = chunks_arg.or(config.chunk_size);

    let generator = config.generator.unwrap_or("random".into());
    let seed = config.seed.unwrap_or_else(|| rand::thread_rng().gen());

    let rng = rand::StdRng::from_seed(&[seed]);
    println!("Used seed {}", seed);

    let transactions: Vec<_> = generate_transactions(
        &generator,
        &mut config.accounts,
        rng,
        config.count,
        config.filter_from.map(AccountId),
        &config.passwords,
    );

    let chunk_size = config.chunk_size.unwrap_or_else(|| transactions.len());

    for (i, chunk) in transactions.chunks(chunk_size).enumerate() {
        let output_file = format!("{}.{}", output_file, i);
        let transactions = chunk;

        let output = File::create(&output_file).expect("Unable to create output file");
        serde_json::to_writer(output, &transactions).expect("Unable to convert to JSON");
        println!("RPC body written to {}", output_file);
    }

    println!("Final balances after {} transactions using the {} generator:", transactions.len(), generator);
    for account in &config.accounts {
        println!("{}:\t{}", account.id.0, account.balance);
    }
}

fn generate_transactions<R>(
    generator_type: &str,
    accounts: &mut [Account],
    mut rng: R,
    count: Option<usize>,
    filter_from: Option<AccountId>,
    passwords: &HashMap<AccountId, Password>,
) -> Vec<PersonalSendTransaction>
where
    R: rand::Rng,
{
    let generator: Box<Iterator<Item = _>> = match generator_type {
        "random" => {
            Box::new(generator::RandomTransactions::new(accounts, &mut rng))
        }
        "winner-loser" => {
            Box::new(generator::WinnerLoser::new(accounts, &mut rng))
        }
        _ => panic!("Unknown generator type {}", generator_type),
    };

    let generator = match count {
        Some(count) => Box::new(generator.take(count)),
        None => generator,
    };

    let generator = match filter_from {
        Some(filter_from) => Box::new(generator.filter(move |&(ref from, _, _)| from == &filter_from)),
        None => generator,
    };

    generator
        .enumerate()
        .map(|(id, (from, to, value))| {
            let password = passwords[&from].clone();
            let transaction = Transaction { from, to, value: format!("0x{:x}", value) };
            let params = PersonalSendTransactionParams(transaction, password);
            PersonalSendTransaction::new(params, id)
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn like_the_wiki() {
        let from = AccountId("0x004ec07d2329997267Ec62b4166639513386F32E".into());
        let to = AccountId("0x00Bd138aBD70e2F00903268F3Db08f2D25677C9e".into());
        let value = "0xde0b6b3a7640000";

        let transaction = Transaction {
            from: from,
            to: to,
            value: value.into(),
        };

        let params = PersonalSendTransactionParams(transaction, Password("user".into()));

        let rpc = vec![
            PersonalSendTransaction::new(params, 0),
        ];

        let actual = serde_json::to_string(&rpc).unwrap();

        let expected = r#"[{"jsonrpc":"2.0","method":"personal_sendTransaction","params":[{"from":"0x004ec07d2329997267Ec62b4166639513386F32E","to":"0x00Bd138aBD70e2F00903268F3Db08f2D25677C9e","value":"0xde0b6b3a7640000"},"user"],"id":0}]"#;
        assert_eq!(actual, expected);
    }

    #[test]
    fn random_transactions() {
        let mut rng = rand::isaac::Isaac64Rng::from_seed(&[1,2,3,4]);

        let mut accounts = vec![
            Account {
                id: AccountId("a".into()),
                balance: 1000,
            },
            Account {
                id: AccountId("b".into()),
                balance: 1000,
            },
        ];

        let transactions: Vec<_> =
            TransactionGenerator::new(&mut accounts, &mut rng)
            .take(10)
            .collect();

        assert_eq!(
            transactions,
            [
                (AccountId("a".into()), AccountId("b".into()), 594),
                (AccountId("b".into()), AccountId("a".into()), 1300),
                (AccountId("b".into()), AccountId("a".into()), 24),
                (AccountId("a".into()), AccountId("b".into()), 1240),
                (AccountId("b".into()), AccountId("a".into()), 1443),
                (AccountId("b".into()), AccountId("a".into()), 42),
                (AccountId("a".into()), AccountId("b".into()), 1347),
                (AccountId("a".into()), AccountId("b".into()), 94),
                (AccountId("b".into()), AccountId("a".into()), 596),
                (AccountId("a".into()), AccountId("b".into()), 503),
            ]
        );
    }
}
