use std::process::{Command, Stdio};

use cba::{broc::CommandExt, env_vars};
use log::info;
use waymaker::{
    AttachmentFormatter, ConfigMMInnerItem, ConfigMMItem, Matchmaker, message::Interrupt,
    use_formatter,
};

#[easy_ext::ext(MMExt)]
impl Matchmaker<ConfigMMItem, ConfigMMInnerItem> {
    /// Causes [`Action::Execute`] to cause the program to execute the program specified by its payload.
    /// Note:
    /// - not intended for direct use.
    /// - Assumes preview and cmd formatter are the same.
    pub fn register_execute_handler(
        &mut self,
        formatter: AttachmentFormatter<ConfigMMItem, ConfigMMInnerItem>,
    ) {
        let formatter_ = formatter.clone();
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            let discriminant = state.discriminant_payload.take();
            let template = state.payload();

            if !template.is_empty() {
                let selected_items = state.map_selected_to_vec(|_, x| x.to_cow().to_string());
                if state.picker_ui.worker.frecency {
                    let store = waymaker::frecency::FrecencyStore::open();
                    for item in &selected_items {
                        let _ = store.add(item);
                    }
                }

                let cmd = use_formatter(&formatter, state, template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = if let Some(Ok(s)) = state.preview_set_payload() {
                    s
                } else {
                    state.preview_payload().clone()
                };
                let preview_cmd = use_formatter(&formatter, state, &preview_template, None);
                let extra = env_vars!(
                    "WM_PREVIEW_COMMAND" => preview_cmd.clone(),
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                if let Some(mut child) = Command::from_script(&cmd)
                    .envs(vars)
                    .stdin(maybe_tty_in())
                    .stdout(maybe_tty_out())
                    .stderr(maybe_tty_out())
                    ._spawn()
                {
                    match child.wait() {
                        Ok(i) => {
                            info!("Command [{cmd}] exited with {i}");
                            match discriminant {
                                // signal termination don't prompt
                                Some(0) if i.code().is_some_and(|c| c != 0) => {
                                    println!("\nPress enter to continue...");
                                    let mut input = String::new();
                                    let _ = std::io::stdin().read_line(&mut input);
                                }
                                Some(1) if i.success() => {
                                    state.should_quit = true;
                                }
                                Some(2) => {
                                    if i.success() {
                                        state.should_quit = true;
                                    } else if i.code().is_some() {
                                        println!("\nPress enter to continue...");
                                        let mut input = String::new();
                                        let _ = std::io::stdin().read_line(&mut input);
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            info!("Failed to wait on command [{cmd}]: {e}")
                        }
                    }
                }
            };
        });

        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let cmd = use_formatter(&formatter_, state, &template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_, state, &preview_template, None);
                let extra = env_vars!(
                    "WM_PREVIEW_COMMAND" => preview_cmd.clone(),
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                if let Some(mut _child) = Command::from_script(&cmd)
                    .envs(vars)
                    .stdin(maybe_tty_in())
                    .stdout(maybe_tty_out())
                    .stderr(maybe_tty_out())
                    ._spawn()
                {
                    // match child.wait() {
                    //     Ok(i) => {
                    //         info!("Command [{cmd}] exited with {i}")
                    //     }
                    //     Err(e) => {
                    //         info!("Failed to wait on command [{cmd}]: {e}")
                    //     }
                    // }
                }
            };
        });
    }
}

pub(crate) fn maybe_tty_in() -> Stdio {
    if let Ok(tty) = std::fs::File::open("/dev/tty") {
        Stdio::from(tty)
    } else {
        log::error!("Failed to open /dev/tty for stdin");
        Stdio::inherit()
    }
}

pub(crate) fn maybe_tty_out() -> Stdio {
    if let Ok(tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        Stdio::from(tty)
    } else {
        log::error!("Failed to open /dev/tty for stdout");
        Stdio::inherit()
    }
}
