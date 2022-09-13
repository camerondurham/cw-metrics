use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::{Arg, Command};

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_cloudwatch::{Client, Error, Region, PKG_VERSION};
use serde::Deserialize;
use tokio::fs;

#[derive(Deserialize, Debug)]
struct AccountsConfig {
    account: Vec<AccountConfig>,
}

#[derive(Deserialize, Debug)]
struct AccountConfig {
    namespace: String,
    account_id: String,
    region: String,
}

#[derive(Debug)]
struct GetWidgetProps {
    region: Option<String>,

    app_name: String,

    title: String,

    verbose: bool,

    template_path: PathBuf,

    start: String,

    end: String,

    period: String,
}

/// Dev CLI for repetitive AWS account tasks
///
/// ## Accounts Config
/// 
/// The accounts are defined in [TOML](https://toml.io) syntax. The file should be a list of tables containing `namespace`, `account_id`, and `region` for each account.
/// 
/// Example (from the repo's accounts.toml):
/// 
/// ```toml
/// [[account]]
/// namespace = "SomeDataProcessingProgram"
/// account_id = "111111111111"
/// region = "us-east-1"
/// ```
/// 
/// To validate accounts config is parsed properly:
/// 
/// ```bash
/// cargo run -- config <ACCOUNT.TOML FILE>
/// 
/// # example
/// cargo run -- config accounts.toml
/// AccountConfig { namespace: "SomeDataProcessingProgram", account_id: "111111111111", region: "us-east-1" }
/// AccountConfig { namespace: "SomeDataProcessingProgram", account_id: "222222222222", region: "eu-west-1" }
/// AccountConfig { namespace: "SomeDataProcessingProgram", account_id: "222222222222", region: "us-west-2" }
/// ...
/// ```
/// 
/// ## Commands
/// 
/// You can use `cargo run --` to build and pass commands to the CLI.
/// 
/// ```bash
/// # run retry counts, replace START_TIME in retry-counts graph to start 6 months ago
/// cargo run -- images --period 3600 --pattern ItemDPP -s 4320H ./resources/traffic.json ../accounts.toml
/// 
/// # omit the pattern to run this command for all accounts
/// cargo run -- images --period 3600  -s 7200H ./resources/traffic.json ../accounts.toml
/// ```

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let matches = Command::new("dev")
        .subcommand(
            Command::new("images")
                .about("download metric widget images from CloudWatch")
                .arg(
                    Arg::new("region")
                        .help("AWS region (e.g. us-east-1, eu-west-1)")
                        .long("region")
                        .short('r')
                        .takes_value(true),
                )
                .arg(
                    Arg::new("start-time")
                        .short('s')
                        .default_value("4320H")
                        .long("start-time")
                        .alias("start")
                        .takes_value(true),
                )
                .arg(
                    Arg::new("end-time")
                        .short('e')
                        .default_value("0H")
                        .alias("end")
                        .takes_value(true),
                )
                .arg(
                    Arg::new("period")
                        .short('p')
                        .default_value("3600")
                        .long("period")
                        .takes_value(true),
                )
                .arg(
                    Arg::new("title")
                        .long("title")
                        .help("title to identify the image downloaded")
                        .default_value("metric")
                        .takes_value(true),
                )
                .arg(Arg::new("template-path").required(true))
                .arg(
                    Arg::new("config-path")
                        .required(true)
                        .help("the path to the TOML config file with accounts"),
                )
                .arg(
                    Arg::new("pattern")
                        .long("pattern")
                        .takes_value(true)
                        .short('f'),
                )
                .arg(
                    Arg::new("output-path")
                        .required(false)
                        .long("output-path")
                        .short('o'),
                ),
        )
        .subcommand(
            Command::new("config")
                .about("validate and display the config file for your accounts")
                .arg(Arg::new("config-path").required(true))
                .arg(
                    Arg::new("pattern")
                        .long("pattern")
                        .takes_value(true)
                        .short('f'),
                ),
        )
        .subcommand(Command::new("show").about("show metrics for an account"))
        .get_matches();

    match matches.subcommand() {
        Some(("images", images)) => {
            let start = images.value_of("start-time").unwrap();
            let end = images.value_of("end-time").unwrap();
            let template_path = images.value_of("template-path").unwrap();
            let period = images.value_of("period").unwrap();
            let title = images.value_of("title").unwrap();
            let config_path = images.value_of("config-path").unwrap();
            let pattern = images.value_of("pattern");
            let accounts = get_accounts(config_path, true);
            let accounts = filter_accounts(pattern, accounts);

            for acc in accounts {
                load_creds(&acc);
                let props = GetWidgetProps {
                    title: String::from(title),
                    region: Some(acc.region),
                    app_name: acc.namespace,
                    template_path: PathBuf::from(template_path),
                    start: String::from(start),
                    end: String::from(end),
                    period: String::from(period),
                    verbose: true,
                };
                match cloudwatch_image_download(props).await {
                    Ok(_) => println!("successful query"),
                    Err(e) => println!("cloudwatch download error: {:?}", e),
                };
            }
        }
        Some(("show", show_matches)) => {
            println!("show: {:?}", show_matches);

            let client = get_client(Some(String::from("us-west-2"))).await;
            let res = show_metrics(&client).await;
            if res.is_err() {
                println!("encountered error getting metrics: {:?}", res.err());
            }
        }
        Some(("config", config)) => {
            let config_path = config.value_of("config-path").unwrap();
            let pattern = config.value_of("pattern");
            let accounts = get_accounts(config_path, true);
            let _filtered = filter_accounts(pattern, accounts);
        }
        _ => unreachable!(),
    };

    Ok(())
}

fn filter_accounts(pattern: Option<&str>, accounts: Option<AccountsConfig>) -> Vec<AccountConfig> {
    if let Some(pat) = pattern {
        let pat = String::from(pat);
        let filtered: Vec<AccountConfig> = accounts
            .unwrap()
            .account
            .into_iter()
            .filter(|x| x.namespace.contains(&pat))
            .collect();
        println!("Filtered accounts:");
        for acc in &filtered {
            println!("{:?}", &acc);
        }
        filtered
    } else {
        accounts.expect("expected accounts to filter").account
    }
}

async fn get_client(region: Option<String>) -> Client {
    let region_provider = RegionProviderChain::first_try(region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    Client::new(&shared_config)
}

async fn cloudwatch_image_download(opts: GetWidgetProps) -> Result<(), Error> {
    let GetWidgetProps {
        title,
        region,
        app_name: namespace,
        verbose,
        template_path: filepath,
        start,
        end,
        period,
    } = opts;

    let replaced_region = region.clone().unwrap_or_else(|| String::from("us-west-2"));

    let region_provider = RegionProviderChain::first_try(region.clone().map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));

    if verbose {
        println!();
        println!("CloudWatch client version: {}", PKG_VERSION);
        println!(
            "Region:                    {}",
            region_provider.region().await.unwrap().as_ref()
        );
        println!();
    }

    // let shared_config = aws_config::from_env().region(region_provider).load().await;
    // let client = Client::new(&shared_config);
    let client = get_client(region).await;
    if let Some(metrics) = get_metrics_json(
        &filepath,
        &replaced_region,
        &namespace,
        &start,
        &end,
        &period,
        verbose,
    ) {
        let saved_image_name = format!(
            "{}-{}-{}-{}-{}",
            &namespace,
            &title,
            &replaced_region,
            &start,
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );
        get_metric_image(&client, metrics.as_ref(), &saved_image_name).await
    } else {
        panic!("unable to parse metrics json")
    }
}

fn get_accounts(filepath: &str, verbose: bool) -> Option<AccountsConfig> {
    let config_file = std::fs::read_to_string(filepath);
    if let Ok(contents) = config_file {
        let accounts_config: AccountsConfig =
            toml::from_str(&contents).expect("unable to parse as toml");
        if verbose {
            // println!("parsed config toml: \n {:?}", &accounts_config);
            for acc in &accounts_config.account {
                println!("{:?}", acc)
            }
        }
        Some(accounts_config)
    } else {
        None
    }
}

fn get_metrics_json(
    filepath: &PathBuf,
    region: &str,
    namespace: &str,
    start: &str,
    end: &str,
    period: &str,
    verbose: bool,
) -> Option<String> {
    let template_file = std::fs::read_to_string(filepath);
    if let Ok(contents) = template_file {
        let mut template_params = HashMap::<&str, &str>::new();

        // TODO: make this configurable
        template_params.insert("{{NAMESPACE}}", namespace);
        template_params.insert("{{REGION}}", region);
        // format: 4320H
        template_params.insert("{{PERIOD_START}}", start);
        template_params.insert("{{PERIOD_END}}", end);
        template_params.insert("{{PERIOD}}", period);

        let mut replaced = contents;
        template_params
            .iter()
            .for_each(|(k, v)| replaced = replaced.replace(k, v));

        if verbose {
            println!("templated:\n{}", &replaced);
        }

        Some(replaced)
    } else {
        None
    }
}

// List metrics.
async fn show_metrics(
    client: &aws_sdk_cloudwatch::Client,
) -> Result<(), aws_sdk_cloudwatch::Error> {
    let rsp = client.list_metrics().send().await?;
    let metrics = rsp.metrics().unwrap_or_default();

    let num_metrics = metrics.len();

    for metric in metrics {
        println!("Namespace: {}", metric.namespace().unwrap_or_default());
        println!("Name:      {}", metric.metric_name().unwrap_or_default());
        println!("Dimensions:");

        if let Some(dimension) = metric.dimensions.as_ref() {
            for d in dimension {
                println!("  Name:  {}", d.name().unwrap_or_default());
                println!("  Value: {}", d.value().unwrap_or_default());
                println!();
            }
        }

        println!();
    }

    println!("Found {} metrics.", num_metrics);

    Ok(())
}

/// Calls AWS CloudWatch GetMetricImage API and downloads locally
/// API Reference: [GetMetricWidgetImage](https://docs.aws.amazon.com/AmazonCloudWatch/latest/APIReference/API_GetMetricWidgetImage.html)
async fn get_metric_image(
    client: &aws_sdk_cloudwatch::Client,
    metric_json: &str,
    saved_image_name: &str,
) -> Result<(), aws_sdk_cloudwatch::Error> {
    println!("getting metric image");

    let request = client
        .get_metric_widget_image()
        .output_format("png")
        .set_metric_widget(Some(String::from(metric_json)));
    let resp = request.send().await?;

    if let Some(blob) = resp.metric_widget_image {
        let path = Path::new(saved_image_name).with_extension("png");

        // convert to base64 encoded byte vector
        let base64_encoded = blob.into_inner();

        // wait to finish saving file
        let res = fs::write(path, base64_encoded).await;
        match res {
            Ok(()) => {
                println!("saved metric image");
            }
            Err(e) => {
                println!("error writing to file: {:?}", e);
            }
        }
    } else {
        println!("error getting metric image");
    }
    Ok(())
}

fn load_creds(account: &AccountConfig) {
	todo!();
}
