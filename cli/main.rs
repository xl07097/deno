// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

mod args;
mod auth_tokens;
mod cache;
mod deno_std;
mod emit;
mod errors;
mod factory;
mod file_fetcher;
mod graph_util;
mod http_util;
mod js;
mod lsp;
mod module_loader;
mod napi;
mod node;
mod npm;
mod ops;
mod resolver;
mod standalone;
mod tools;
mod tsc;
mod util;
mod version;
mod watcher;
mod worker;

use crate::args::flags_from_vec;
use crate::args::DenoSubcommand;
use crate::args::Flags;
use crate::util::display;
use crate::util::v8::get_v8_flags_from_env;
use crate::util::v8::init_v8_flags;

use args::CliOptions;
use deno_core::anyhow::Context;
use deno_core::error::AnyError;
use deno_core::error::JsError;
use deno_core::futures::FutureExt;
use deno_core::task::JoinHandle;
use deno_runtime::colors;
use deno_runtime::fmt_errors::format_js_error;
use deno_runtime::tokio_util::create_and_run_current_thread;
use factory::CliFactory;
use std::env;
use std::env::current_exe;
use std::future::Future;
use std::path::PathBuf;

/// Ensures that all subcommands return an i32 exit code and an [`AnyError`] error type.
trait SubcommandOutput {
  fn output(self) -> Result<i32, AnyError>;
}

impl SubcommandOutput for Result<i32, AnyError> {
  fn output(self) -> Result<i32, AnyError> {
    self
  }
}

impl SubcommandOutput for Result<(), AnyError> {
  fn output(self) -> Result<i32, AnyError> {
    self.map(|_| 0)
  }
}

impl SubcommandOutput for Result<(), std::io::Error> {
  fn output(self) -> Result<i32, AnyError> {
    self.map(|_| 0).map_err(|e| e.into())
  }
}

/// Ensure that the subcommand runs in a task, rather than being directly executed. Since some of these
/// futures are very large, this prevents the stack from getting blown out from passing them by value up
/// the callchain (especially in debug mode when Rust doesn't have a chance to elide copies!).
#[inline(always)]
fn spawn_subcommand<F: Future<Output = T> + 'static, T: SubcommandOutput>(
  f: F,
) -> JoinHandle<Result<i32, AnyError>> {
  deno_core::task::spawn(f.map(|r| r.output()))
}

async fn run_subcommand(flags: Flags) -> Result<i32, AnyError> {
  let handle = match flags.subcommand.clone() {
    DenoSubcommand::Bench(bench_flags) => spawn_subcommand(async {
      let cli_options = CliOptions::from_flags(flags)?;
      let bench_options = cli_options.resolve_bench_options(bench_flags)?;
      if cli_options.watch_paths().is_some() {
        tools::bench::run_benchmarks_with_watch(cli_options, bench_options)
          .await
      } else {
        tools::bench::run_benchmarks(cli_options, bench_options).await
      }
    }),
    DenoSubcommand::Bundle(bundle_flags) => spawn_subcommand(async {
      tools::bundle::bundle(flags, bundle_flags).await
    }),
    DenoSubcommand::Doc(doc_flags) => {
      spawn_subcommand(async { tools::doc::print_docs(flags, doc_flags).await })
    }
    DenoSubcommand::Eval(eval_flags) => spawn_subcommand(async {
      tools::run::eval_command(flags, eval_flags).await
    }),
    DenoSubcommand::Cache(cache_flags) => spawn_subcommand(async move {
      let factory = CliFactory::from_flags(flags).await?;
      let module_load_preparer = factory.module_load_preparer().await?;
      let emitter = factory.emitter()?;
      let graph_container = factory.graph_container();
      module_load_preparer
        .load_and_type_check_files(&cache_flags.files)
        .await?;
      emitter.cache_module_emits(&graph_container.graph())
    }),
    DenoSubcommand::Check(check_flags) => spawn_subcommand(async move {
      let factory = CliFactory::from_flags(flags).await?;
      let module_load_preparer = factory.module_load_preparer().await?;
      module_load_preparer
        .load_and_type_check_files(&check_flags.files)
        .await
    }),
    DenoSubcommand::Compile(compile_flags) => spawn_subcommand(async {
      tools::compile::compile(flags, compile_flags).await
    }),
    DenoSubcommand::Coverage(coverage_flags) => spawn_subcommand(async {
      tools::coverage::cover_files(flags, coverage_flags).await
    }),
    DenoSubcommand::Fmt(fmt_flags) => spawn_subcommand(async move {
      let cli_options = CliOptions::from_flags(flags.clone())?;
      let fmt_options = cli_options.resolve_fmt_options(fmt_flags)?;
      tools::fmt::format(cli_options, fmt_options).await
    }),
    DenoSubcommand::Init(init_flags) => {
      spawn_subcommand(async { tools::init::init_project(init_flags).await })
    }
    DenoSubcommand::Info(info_flags) => {
      spawn_subcommand(async { tools::info::info(flags, info_flags).await })
    }
    DenoSubcommand::Install(install_flags) => spawn_subcommand(async {
      tools::installer::install_command(flags, install_flags).await
    }),
    DenoSubcommand::Uninstall(uninstall_flags) => spawn_subcommand(async {
      tools::installer::uninstall(uninstall_flags.name, uninstall_flags.root)
    }),
    DenoSubcommand::Lsp => spawn_subcommand(async { lsp::start().await }),
    DenoSubcommand::Lint(lint_flags) => spawn_subcommand(async {
      if lint_flags.rules {
        tools::lint::print_rules_list(lint_flags.json);
        Ok(())
      } else {
        let cli_options = CliOptions::from_flags(flags)?;
        let lint_options = cli_options.resolve_lint_options(lint_flags)?;
        tools::lint::lint(cli_options, lint_options).await
      }
    }),
    DenoSubcommand::Repl(repl_flags) => {
      spawn_subcommand(async move { tools::repl::run(flags, repl_flags).await })
    }
    DenoSubcommand::Run(run_flags) => spawn_subcommand(async move {
      if run_flags.is_stdin() {
        tools::run::run_from_stdin(flags).await
      } else {
        tools::run::run_script(flags).await
      }
    }),
    DenoSubcommand::Task(task_flags) => spawn_subcommand(async {
      tools::task::execute_script(flags, task_flags).await
    }),
    DenoSubcommand::Test(test_flags) => {
      spawn_subcommand(async {
        if let Some(ref coverage_dir) = flags.coverage_dir {
          std::fs::create_dir_all(coverage_dir)
            .with_context(|| format!("Failed creating: {coverage_dir}"))?;
          // this is set in order to ensure spawned processes use the same
          // coverage directory
          env::set_var(
            "DENO_UNSTABLE_COVERAGE_DIR",
            PathBuf::from(coverage_dir).canonicalize()?,
          );
        }
        let cli_options = CliOptions::from_flags(flags)?;
        let test_options = cli_options.resolve_test_options(test_flags)?;

        if cli_options.watch_paths().is_some() {
          tools::test::run_tests_with_watch(cli_options, test_options).await
        } else {
          tools::test::run_tests(cli_options, test_options).await
        }
      })
    }
    DenoSubcommand::Completions(completions_flags) => {
      spawn_subcommand(async move {
        display::write_to_stdout_ignore_sigpipe(&completions_flags.buf)
      })
    }
    DenoSubcommand::Types => spawn_subcommand(async move {
      let types = tsc::get_types_declaration_file_text(flags.unstable);
      display::write_to_stdout_ignore_sigpipe(types.as_bytes())
    }),
    DenoSubcommand::Upgrade(upgrade_flags) => spawn_subcommand(async {
      tools::upgrade::upgrade(flags, upgrade_flags).await
    }),
    DenoSubcommand::Vendor(vendor_flags) => spawn_subcommand(async {
      tools::vendor::vendor(flags, vendor_flags).await
    }),
  };

  handle.await?
}

fn setup_panic_hook() {
  // This function does two things inside of the panic hook:
  // - Tokio does not exit the process when a task panics, so we define a custom
  //   panic hook to implement this behaviour.
  // - We print a message to stderr to indicate that this is a bug in Deno, and
  //   should be reported to us.
  let orig_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |panic_info| {
    eprintln!("\n============================================================");
    eprintln!("Deno has panicked. This is a bug in Deno. Please report this");
    eprintln!("at https://github.com/denoland/deno/issues/new.");
    eprintln!("If you can reliably reproduce this panic, include the");
    eprintln!("reproduction steps and re-run with the RUST_BACKTRACE=1 env");
    eprintln!("var set and include the backtrace in your report.");
    eprintln!();
    eprintln!("Platform: {} {}", env::consts::OS, env::consts::ARCH);
    eprintln!("Version: {}", version::deno());
    eprintln!("Args: {:?}", env::args().collect::<Vec<_>>());
    eprintln!();
    orig_hook(panic_info);
    std::process::exit(1);
  }));
}

fn unwrap_or_exit<T>(result: Result<T, AnyError>) -> T {
  match result {
    Ok(value) => value,
    Err(error) => {
      let mut error_string = format!("{error:?}");
      let mut error_code = 1;

      if let Some(e) = error.downcast_ref::<JsError>() {
        error_string = format_js_error(e);
      } else if let Some(e) = error.downcast_ref::<args::LockfileError>() {
        error_string = e.to_string();
        error_code = 10;
      }

      eprintln!(
        "{}: {}",
        colors::red_bold("error"),
        error_string.trim_start_matches("error: ")
      );
      std::process::exit(error_code);
    }
  }
}

pub fn main() {
  setup_panic_hook();

  util::unix::raise_fd_limit();
  util::windows::ensure_stdio_open();
  #[cfg(windows)]
  colors::enable_ansi(); // For Windows 10
  deno_runtime::permissions::set_prompt_callbacks(
    Box::new(util::draw_thread::DrawThread::hide),
    Box::new(util::draw_thread::DrawThread::show),
  );

  let args: Vec<String> = env::args().collect();

  let future = async move {
    let current_exe_path = current_exe()?;
    let standalone_res =
      match standalone::extract_standalone(&current_exe_path, args.clone())
        .await
      {
        Ok(Some((metadata, eszip))) => standalone::run(eszip, metadata).await,
        Ok(None) => Ok(()),
        Err(err) => Err(err),
      };
    // TODO(bartlomieju): doesn't handle exit code set by the runtime properly
    unwrap_or_exit(standalone_res);

    let flags = match flags_from_vec(args) {
      Ok(flags) => flags,
      Err(err @ clap::Error { .. })
        if err.kind() == clap::error::ErrorKind::DisplayHelp
          || err.kind() == clap::error::ErrorKind::DisplayVersion =>
      {
        err.print().unwrap();
        std::process::exit(0);
      }
      Err(err) => unwrap_or_exit(Err(AnyError::from(err))),
    };

    let default_v8_flags = match flags.subcommand {
      // Using same default as VSCode:
      // https://github.com/microsoft/vscode/blob/48d4ba271686e8072fc6674137415bc80d936bc7/extensions/typescript-language-features/src/configuration/configuration.ts#L213-L214
      DenoSubcommand::Lsp => vec!["--max-old-space-size=3072".to_string()],
      _ => vec![],
    };
    init_v8_flags(&default_v8_flags, &flags.v8_flags, get_v8_flags_from_env());

    util::logger::init(flags.log_level);

    run_subcommand(flags).await
  };

  let exit_code = unwrap_or_exit(create_and_run_current_thread(future));

  std::process::exit(exit_code);
}
