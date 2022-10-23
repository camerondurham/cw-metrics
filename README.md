Dumb dev CLI for repetitive tasks of gathering metrics across many AWS accounts.

(wrote this when I had to get metrics across 42 accounts :P)

                                                                                                                                                                     
## Accounts Config

The accounts are defined in [TOML](https://toml.io). The file should be a list of tables containing `namespace`, `account_id`, and `region` for each account.

Example (from the repo's accounts.toml):

```toml
[[account]]
namespace = "SomeDataProcessingProgram"
account_id = "111111111111"
region = "us-east-1"
```

To validate accounts config is parsed properly:

```bash
cargo run -- config <ACCOUNT.TOML FILE>

# example
cargo run -- config accounts.toml
AccountConfig { namespace: "SomeDataProcessingProgram", account_id: "111111111111", region: "us-east-1" }
AccountConfig { namespace: "SomeDataProcessingProgram", account_id: "222222222222", region: "eu-west-1" }
AccountConfig { namespace: "SomeDataProcessingProgram", account_id: "222222222222", region: "us-west-2" }
...
```

## Commands

You can use `cargo run --` to build and pass commands to the CLI.

```bash
# run retry counts, replace START_TIME in retry-counts graph to start 6 months ago
cargo run -- images --period 3600 --pattern ItemDPP -s 4320H ./resources/traffic.json ./accounts.toml

# omit the pattern to run this command for all accounts
cargo run -- images --period 3600  -s 7200H ./resources/traffic.json ./accounts.toml
```

## Future work

- Calculate exact metric statistics with [GetMetricStatistics](https://docs.rs/aws-sdk-cloudwatch/latest/aws_sdk_cloudwatch/client/fluent_builders/struct.GetMetricStatistics.html)
