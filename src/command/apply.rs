use clap::{Arg, App, SubCommand, ArgMatches};

use crate::nix::{DeploymentTask, DeploymentGoal};
use crate::nix::host::CopyOptions;
use crate::deployment::deploy;
use crate::util;

pub fn subcommand() -> App<'static, 'static> {
    let command = SubCommand::with_name("apply")
        .about("Apply configurations on remote machines")
        .arg(Arg::with_name("goal")
            .help("Deployment goal")
            .long_help("Same as the targets for switch-to-configuration.\n\"push\" means only copying the closures to remote nodes.")
            .default_value("switch")
            .index(1)
            .possible_values(&["push", "switch", "boot", "test", "dry-activate"]))
        .arg(Arg::with_name("parallel")
            .short("p")
            .long("parallel")
            .value_name("LIMIT")
            .help("Parallelism limit")
            .long_help(r#"Limits the maximum number of hosts to be deployed in parallel.

Set to 0 to disable parallemism limit.
"#)
            .default_value("10")
            .takes_value(true)
            .validator(|s| {
                match s.parse::<usize>() {
                    Ok(_) => Ok(()),
                    Err(_) => Err(String::from("The value must be a valid number")),
                }
            }))
        .arg(Arg::with_name("verbose")
            .short("v")
            .long("verbose")
            .help("Be verbose")
            .long_help("Deactivates the progress spinner and prints every line of output.")
            .takes_value(false))
        .arg(Arg::with_name("no-substitutes")
            .long("no-substitutes")
            .help("Do not use substitutes")
            .long_help("Disables the use of substituters when copying closures to the remote host.")
            .takes_value(false))
        .arg(Arg::with_name("no-gzip")
            .long("no-gzip")
            .help("Do not use gzip")
            .long_help("Disables the use of gzip when copying closures to the remote host.")
            .takes_value(false))
    ;

    util::register_selector_args(command)
}

pub async fn run(_global_args: &ArgMatches<'_>, local_args: &ArgMatches<'_>) {
    let mut hive = util::hive_from_args(local_args).unwrap();

    log::info!("Enumerating nodes...");
    let all_nodes = hive.deployment_info().await.unwrap();

    let selected_nodes = match local_args.value_of("on") {
        Some(filter) => {
            util::filter_nodes(&all_nodes, filter)
        }
        None => all_nodes.keys().cloned().collect(),
    };

    if selected_nodes.len() == 0 {
        log::warn!("No hosts matched. Exiting...");
        quit::with_code(2);
    }

    if selected_nodes.len() == all_nodes.len() {
        log::info!("Building all node configurations...");
    } else {
        log::info!("Selected {} out of {} hosts. Building node configurations...", selected_nodes.len(), all_nodes.len());
    }

    // Some ugly argument mangling :/
    let mut profiles = hive.build_selected(selected_nodes).await.unwrap();
    let goal = DeploymentGoal::from_str(local_args.value_of("goal").unwrap()).unwrap();
    let verbose = local_args.is_present("verbose");

    let max_parallelism = local_args.value_of("parallel").unwrap().parse::<usize>().unwrap();
    let max_parallelism = match max_parallelism {
        0 => None,
        _ => Some(max_parallelism),
    };

    let mut task_list: Vec<DeploymentTask> = Vec::new();
    let mut skip_list: Vec<String> = Vec::new();
    for (name, profile) in profiles.drain() {
        let target = all_nodes.get(&name).unwrap().to_ssh_host();

        match target {
            Some(target) => {
                let mut task = DeploymentTask::new(name, target, profile, goal);
                let options = CopyOptions::default()
                    .gzip(!local_args.is_present("no-gzip"))
                    .use_substitutes(!local_args.is_present("no-substitutes"))
                ;

                task.set_copy_options(options);
                task_list.push(task);
            }
            None => {
                skip_list.push(name);
            }
        }
    }

    if skip_list.len() != 0 {
        log::info!("Applying configurations ({} skipped)...", skip_list.len());
    } else {
        log::info!("Applying configurations...");
    }

    deploy(task_list, max_parallelism, !verbose).await;
}
