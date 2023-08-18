/*
 * Copyright 2023 ByteDance and/or its affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::path::PathBuf;
use std::str::FromStr;

use clap::{value_parser, Arg, ArgMatches, Command, ValueHint};

use g3proxy_proto::proc_capnp::proc_control;
use g3proxy_proto::user_group_capnp::user_group_control;

use super::common::{parse_operation_result, print_list_text};
use super::{CommandError, CommandResult};

pub const COMMAND: &str = "user-group";

const COMMAND_ARG_NAME: &str = "name";
const COMMAND_ARG_FILE: &str = "file";

const SUBCOMMAND_LIST_STATIC_USER: &str = "list-static-user";
const SUBCOMMAND_LIST_DYNAMIC_USER: &str = "list-dynamic-user";
const SUBCOMMAND_PUBLISH_DYNAMIC_USER: &str = "publish-dynamic-user";

pub fn command() -> Command {
    Command::new(COMMAND)
        .arg(Arg::new(COMMAND_ARG_NAME).required(true).num_args(1))
        .subcommand(Command::new(SUBCOMMAND_LIST_STATIC_USER))
        .subcommand(Command::new(SUBCOMMAND_LIST_DYNAMIC_USER))
        .subcommand(
            Command::new(SUBCOMMAND_PUBLISH_DYNAMIC_USER)
                .long_flag("publish")
                .arg(
                    Arg::new(COMMAND_ARG_FILE)
                        .required(true)
                        .num_args(1)
                        .value_parser(value_parser!(PathBuf))
                        .value_hint(ValueHint::FilePath),
                ),
        )
}

pub async fn run(client: &proc_control::Client, args: &ArgMatches) -> CommandResult<()> {
    let name = args.get_one::<String>(COMMAND_ARG_NAME).unwrap();

    let user_group = super::proc::get_user_group(client, name).await?;

    if let Some((subcommand, args)) = args.subcommand() {
        match subcommand {
            SUBCOMMAND_LIST_STATIC_USER => list_static_user(&user_group).await,
            SUBCOMMAND_LIST_DYNAMIC_USER => list_dynamic_user(&user_group).await,
            SUBCOMMAND_PUBLISH_DYNAMIC_USER => publish_dynamic_user(&user_group, args).await,
            cmd => Err(CommandError::Cli(format!("unsupported subcommand {cmd}"))),
        }
    } else {
        Ok(())
    }
}

async fn list_static_user(client: &user_group_control::Client) -> CommandResult<()> {
    let req = client.list_static_user_request();
    let rsp = req.send().promise.await?;
    print_list_text(rsp.get()?.get_result()?)
}

async fn list_dynamic_user(client: &user_group_control::Client) -> CommandResult<()> {
    let req = client.list_dynamic_user_request();
    let rsp = req.send().promise.await?;
    print_list_text(rsp.get()?.get_result()?)
}

async fn publish_dynamic_user(
    client: &user_group_control::Client,
    args: &ArgMatches,
) -> CommandResult<()> {
    let data = if let Some(file) = args.get_one::<PathBuf>(COMMAND_ARG_FILE) {
        tokio::fs::read_to_string(file).await.map_err(|e| {
            CommandError::Cli(format!(
                "failed to read contents of file {}: {e:?}",
                file.display()
            ))
        })?
    } else {
        unreachable!()
    };

    if let Err(e) = serde_json::Value::from_str(&data) {
        return Err(CommandError::Cli(format!(
            "the data to publish is not valid json: {e:?}"
        )));
    }

    let mut req = client.publish_dynamic_user_request();
    req.get().set_contents(&data);
    let rsp = req.send().promise.await?;
    parse_operation_result(rsp.get()?.get_result()?)
}
