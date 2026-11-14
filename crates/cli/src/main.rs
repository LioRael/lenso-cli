use lenso_cli::Cli;
mod self_update;
mod update_notice;

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if update_notice::is_worker(&args) {
        update_notice::worker();
    } else {
        run(args);
    }
}

#[tokio::main]
async fn run(args: Vec<std::ffi::OsString>) {
    let notify = update_notice::enabled(&args);
    let setup_args = args.clone();
    let setup = Cli::prepare(&args, move |events| {
        let args = setup_args;
        let mut options = lenso_runtime::LoadOptions::from_args(&args);
        options.events = events;
        #[cfg(feature = "ops")]
        options.providers.insert(
            "ops-host".into(),
            lenso_runtime::NativeProvider {
                version: lenso_ops::VERSION.into(),
                register: lenso_ops::register,
            },
        );
        #[cfg(feature = "project")]
        options.providers.insert(
            "project".into(),
            lenso_runtime::NativeProvider {
                version: lenso_project::VERSION.into(),
                register: lenso_project::register,
            },
        );
        let mut cli = Cli::new("lenso");
        self_update::register(&mut cli, options.offline)?;
        lenso_runtime::register_management_with_options(&mut cli, &options)?;
        let version_only = Cli::command_from_args(&args).is_none()
            && args.iter().any(|arg| arg == "--version" || arg == "-V");
        if !version_only
            && !matches!(
                Cli::command_from_args(&args),
                Some(
                    "new"
                        | "self"
                        | "init"
                        | "ext"
                        | "preset"
                        | "update"
                        | "config"
                        | "migrate"
                        | "cache"
                        | "package"
                        | "publish"
                )
            )
        {
            lenso_runtime::load_project_with_options(
                &mut cli,
                &Cli::project_from_args(&args)?,
                &options,
            )?;
        }
        Ok::<_, lenso_cli::Error>(cli)
    })
    .await;
    let code = match setup {
        Ok(cli) => cli.run_from(args).await,
        Err(error) => {
            Cli::diagnostic(&error, &args);
            1
        }
    };
    if code == 0 && notify {
        update_notice::after_command();
    }
    std::process::exit(code);
}
