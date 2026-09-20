use super::*;
use crate::updates::{Action, Close, Notice};
use towavue_core::release::ReleaseVersion;
use towavue_runtime_windows::update::{UpdateEvent, UpdatePhase, UpdateService};

const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Guards,
    Preparing,
    Committing,
    Exiting,
}

struct Attempt {
    token: u64,
    step: Step,
}

#[derive(Default)]
pub(super) struct Updates {
    service: Option<UpdateService>,
    pub(super) worker_epoch: u64,
    shutdown_requested: bool,
    pub(super) startup: bool,
    enabled: bool,
    unavailable: Option<String>,
    checking: bool,
    manual: bool,
    next_check: Option<Instant>,
    notice: Option<Notice>,
    attempt: Option<Attempt>,
    next_token: u64,
    cancelling: Option<u64>,
    deferring: bool,
    #[cfg(test)]
    headless_prompt: bool,
}

impl WindowHost {
    /// Called by main only after launch forwarding elects this process primary.
    /// Tests and evaluation tools constructing WindowHost do not start networking.
    pub(crate) fn start_updates(
        &mut self,
        initial_path: Option<PathBuf>,
    ) -> Result<(), Box<dyn Error>> {
        self.start_update_worker(initial_path, true)
    }

    fn start_update_worker(
        &mut self,
        initial_path: Option<PathBuf>,
        fresh_primary: bool,
    ) -> Result<(), Box<dyn Error>> {
        let proxy = self.proxy.clone().ok_or("Missing update event proxy")?;
        let version: ReleaseVersion = env!("CARGO_PKG_VERSION").parse()?;
        self.updates.worker_epoch = self
            .updates
            .worker_epoch
            .checked_add(1)
            .expect("update worker identity exhausted");
        let epoch = self.updates.worker_epoch;
        self.updates.service = Some(UpdateService::start(version, initial_path, move |event| {
            let _ = proxy.send_event(Event::Update(epoch, event));
        })?);
        self.updates.startup = fresh_primary;
        self.updates.shutdown_requested = false;
        self.updates.enabled = false;
        self.updates.unavailable = None;
        self.updates.checking = false;
        self.updates.manual = false;
        self.updates.cancelling = None;
        self.updates.deferring = false;
        self.updates.notice = None;
        self.updates.next_check = None;
        Ok(())
    }

    fn update_status(&mut self, message: impl Into<String>) {
        let message = message.into();
        for app in self.windows.values_mut().filter(|app| !app.exit_requested) {
            app.set_status(message.clone());
            app.request_redraw();
        }
    }

    fn check_updates(&mut self, manual: bool) {
        if !self.updates.enabled {
            if manual {
                self.update_status(self.updates.unavailable.clone().unwrap_or_else(|| {
                    "Automatic updates are available in an installed production copy.".into()
                }));
            }
            return;
        }
        if self.updates.checking
            || self.updates.attempt.is_some()
            || self.updates.cancelling.is_some()
            || self.updates.deferring
        {
            return;
        }
        self.updates.checking = true;
        self.updates.manual = manual;
        self.updates.next_check = Some(Instant::now() + CHECK_INTERVAL);
        if let Some(service) = &self.updates.service {
            service.check(manual);
        }
        if manual {
            self.update_status("Checking for updates…");
        }
    }

    pub(super) fn update_choice(&mut self, origin: WindowKey, action: Action) {
        if !self.windows.contains_key(&origin) {
            return;
        }
        match action {
            Action::Check => self.check_updates(true),
            Action::Dismiss => {
                self.updates.notice = None;
                for app in self.windows.values_mut() {
                    app.update_notice = None;
                }
            }
            Action::Cancel => self.cancel_update("Update cancelled. Your windows remain open."),
            Action::Install | Action::NextLaunch => {
                if self.updates.notice.is_none()
                    || self.updates.attempt.is_some()
                    || self.updates.cancelling.is_some()
                    || self.updates.deferring
                {
                    return;
                }
                for app in self.windows.values_mut() {
                    app.update_notice = None;
                }
                if action == Action::NextLaunch {
                    self.updates.deferring = true;
                    if let Some(service) = &self.updates.service {
                        service.defer();
                    }
                } else {
                    self.begin_update(false);
                }
            }
        }
    }

    fn begin_update(&mut self, startup: bool) {
        if self.updates.attempt.is_some() || self.updates.cancelling.is_some() {
            return;
        }
        if !startup
            && (self.file_operation.is_some()
                || self.source_save.is_some()
                || !self.pending_launches.is_empty()
                || self.windows.values().any(|app| !app.update_can_prompt()))
        {
            self.update_status(
                "Finish the active operation in every window before installing the update.",
            );
            return;
        }
        self.updates.next_token = self
            .updates
            .next_token
            .checked_add(1)
            .expect("update identity exhausted");
        let token = self.updates.next_token;
        self.updates.attempt = Some(Attempt {
            token,
            step: Step::Guards,
        });
        self.updates.notice = None;
        for app in self.windows.values_mut() {
            app.update_notice = None;
            app.update_close = Some(Close {
                token,
                approved: None,
                committing: false,
            });
            app.request_guarded(GuardedAction::UpdateExit(token));
        }
        self.advance_update();
    }

    fn update_approvals_current(&self, token: u64) -> bool {
        !self.windows.is_empty()
            && self.file_operation.is_none()
            && self.source_save.is_none()
            && self.pending_launches.is_empty()
            && self.windows.values().all(|app| {
                !app.exit_requested
                    && app.active_export.is_none()
                    && app.pending_dialog.is_none()
                    && app.native_prompt.is_none()
                    && app.pending_guard.is_none()
                    && app.pending_window_open.is_none()
                    && app.pending_window_launches.is_empty()
                    && app.update_close.as_ref().is_some_and(|close| {
                        close.token == token && close.approved.as_ref() == Some(&app.edits)
                    })
            })
    }

    pub(super) fn cancel_update(&mut self, reason: &str) {
        let Some(attempt) = self.updates.attempt.take() else {
            return;
        };
        if matches!(attempt.step, Step::Committing | Step::Exiting) {
            self.updates.attempt = Some(attempt);
            return;
        }
        if let Some(service) = &mut self.updates.service {
            service.cancel_install(attempt.token);
        }
        self.updates.cancelling = Some(attempt.token);
        self.updates.startup = false;
        for app in self.windows.values_mut() {
            if app
                .update_close
                .as_ref()
                .is_some_and(|close| close.token == attempt.token)
            {
                app.update_close = None;
                if matches!(app.pending_guard, Some(GuardedAction::UpdateExit(_))) {
                    app.pending_guard = None;
                }
                // A user-approved save may finish; UpdateExit carries the old
                // token and cannot close or approve a later update attempt.
            }
        }
        self.update_status(reason);
    }

    pub(super) fn update_event(&mut self, event: UpdateEvent) {
        match event {
            UpdateEvent::Disabled => {
                self.updates.startup = false;
                self.updates.enabled = false;
                self.updates.checking = false;
                self.updates.manual = false;
                self.updates.next_check = None;
            }
            UpdateEvent::Unavailable(message) => {
                self.updates.startup = false;
                self.updates.enabled = false;
                self.updates.checking = false;
                self.updates.manual = false;
                self.updates.next_check = None;
                let message = format!("Update unavailable: {message}");
                self.updates.unavailable = Some(message.clone());
                self.update_status(message);
            }
            UpdateEvent::StartupComplete => {
                self.updates.startup = false;
                self.updates.enabled = true;
                self.check_updates(false);
            }
            UpdateEvent::InstallationInProgress => {
                self.updates.startup = false;
                self.updates.enabled = false;
                // A helper is holding this installation. Do not open another
                // executable instance while it waits to replace the old files.
                for app in self.windows.values_mut() {
                    app.exit_requested = true;
                }
            }
            UpdateEvent::Ready {
                version,
                phase,
                startup,
            } => {
                self.updates.enabled = true;
                self.updates.checking = false;
                self.updates.manual = false;
                self.updates.next_check = Some(Instant::now() + CHECK_INTERVAL);
                if startup && self.updates.startup && phase == UpdatePhase::NextLaunch {
                    self.begin_update(true);
                } else {
                    self.updates.startup = false;
                    self.updates.notice = Some(Notice {
                        version,
                        failed: phase == UpdatePhase::Failed,
                    });
                }
            }
            UpdateEvent::Current => {
                self.updates.checking = false;
                if self.updates.manual {
                    self.update_status("No new update is available.");
                }
                self.updates.manual = false;
            }
            UpdateEvent::Deferred => {
                self.updates.deferring = false;
                self.updates.notice = None;
                self.update_status("The update will install on the next launch.");
            }
            UpdateEvent::HandoffReady(token) => {
                if self
                    .updates
                    .attempt
                    .as_ref()
                    .is_some_and(|a| a.token == token && a.step == Step::Preparing)
                    && self.update_approvals_current(token)
                {
                    self.updates.attempt.as_mut().expect("current attempt").step = Step::Committing;
                    for app in self.windows.values_mut() {
                        if let Some(close) = &mut app.update_close {
                            close.committing = true;
                        }
                        app.request_redraw();
                    }
                    if let Some(service) = &self.updates.service {
                        service.commit(token);
                    }
                } else {
                    if self
                        .updates
                        .attempt
                        .as_ref()
                        .is_some_and(|a| a.token == token)
                    {
                        self.cancel_update("Update cancelled because a window changed.");
                    }
                    if let Some(service) = &mut self.updates.service {
                        service.cancel_install(token);
                    }
                }
            }
            UpdateEvent::Committed(token) => {
                if let Some(attempt) = &mut self.updates.attempt
                    && attempt.token == token
                    && attempt.step == Step::Committing
                {
                    attempt.step = Step::Exiting;
                    self.updates.startup = false;
                    for app in self.windows.values_mut() {
                        app.exit_requested = true;
                    }
                }
            }
            UpdateEvent::Cancelled(token) => {
                if self.updates.cancelling == Some(token) {
                    self.updates.cancelling = None;
                }
            }
            UpdateEvent::Error {
                message,
                startup,
                operation,
            } => {
                if let Some(token) = operation {
                    if self.updates.cancelling == Some(token) {
                        self.updates.cancelling = None;
                    }
                    if self
                        .updates
                        .attempt
                        .as_ref()
                        .is_some_and(|a| a.token == token)
                    {
                        // Commit failed before helper detachment; the worker has
                        // already dropped it. Restore the application as a unit.
                        self.updates.attempt.as_mut().expect("current attempt").step =
                            Step::Preparing;
                        self.cancel_update("Update could not start.");
                    }
                } else {
                    self.updates.checking = false;
                    self.updates.deferring = false;
                }
                if startup {
                    self.updates.startup = false;
                    self.updates.enabled = true;
                    self.updates.next_check = Some(Instant::now() + CHECK_INTERVAL);
                }
                self.updates.manual = false;
                self.update_status(format!("Update unavailable: {message}"));
            }
        }
        self.advance_update();
    }

    pub(super) fn advance_update(&mut self) {
        if let Some(attempt) = &self.updates.attempt {
            let token = attempt.token;
            let step = attempt.step;
            if matches!(step, Step::Committing | Step::Exiting) {
                return;
            }
            let invalid = self.windows.is_empty()
                || self.windows.values().any(|app| {
                    app.exit_requested
                        || app
                            .active_export
                            .as_ref()
                            .is_some_and(|export| export.cancelling)
                        || app.update_close.as_ref().is_none_or(|close| {
                            close.token != token
                                || close
                                    .approved
                                    .as_ref()
                                    .is_some_and(|edits| edits != &app.edits)
                                || close.approved.is_none()
                                    && app.pending_guard.is_none()
                                    && app.pending_dialog.is_none()
                                    && app.native_prompt.is_none()
                                    && app.active_export.is_none()
                        })
                });
            if invalid {
                self.cancel_update(
                    "Update cancelled because a window changed or a save was cancelled.",
                );
            } else if step == Step::Guards && self.update_approvals_current(token) {
                self.updates.attempt.as_mut().expect("current attempt").step = Step::Preparing;
                if let Some(service) = &mut self.updates.service {
                    service.prepare(token);
                }
            }
            return;
        }
        if self.updates.startup || self.updates.cancelling.is_some() || self.updates.deferring {
            return;
        }
        if let Some(notice) = self.updates.notice {
            #[cfg(not(test))]
            let headless_prompt = false;
            #[cfg(test)]
            let headless_prompt = self.updates.headless_prompt;
            let shown = self
                .windows
                .values()
                .any(|app| app.update_notice.is_some() && !app.exit_requested);
            if !shown
                && self.file_operation.is_none()
                && self.source_save.is_none()
                && let Some(app) = self.windows.values_mut().find(|app| {
                    (app.window.is_some() || headless_prompt) && app.update_can_prompt()
                })
            {
                app.update_notice = Some(notice);
                app.open_native_prompt(FallbackPrompt::UpdateNotice(notice));
                app.request_redraw();
            }
        }
        if self
            .updates
            .next_check
            .is_some_and(|next| Instant::now() >= next)
        {
            self.check_updates(false);
        }
    }

    pub(super) fn update_wait(&mut self) -> ControlFlow {
        if self.windows.is_empty() {
            if !self.updates.shutdown_requested
                && let Some(service) = &mut self.updates.service
            {
                service.shutdown();
                self.updates.shutdown_requested = true;
                self.updates.enabled = false;
                self.updates.checking = false;
                self.updates.next_check = None;
            }
            return exit_wait(self.update_worker_pending());
        }
        // Launch forwarding can reopen the host during final Shell/worker
        // retirement. Wait for the old worker, then resume checking without
        // treating this as a fresh primary launch or consuming NextLaunch.
        if self.updates.shutdown_requested {
            if self.update_worker_pending() {
                return ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(10));
            }
            self.updates.shutdown_requested = false;
            if let Err(error) = self.start_update_worker(None, false) {
                self.update_status(format!("Update unavailable: {error}"));
            }
        }
        if !self.updates.enabled
            || self.updates.checking
            || self.updates.attempt.is_some()
            || self.updates.cancelling.is_some()
            || self.updates.deferring
        {
            ControlFlow::Wait
        } else {
            self.updates
                .next_check
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
        }
    }

    pub(super) fn clear_stale_update_guards(&mut self) {
        for app in self.windows.values_mut() {
            if matches!(app.pending_guard, Some(GuardedAction::UpdateExit(token)) if app.update_close.as_ref().is_none_or(|close| close.token != token))
            {
                app.pending_guard = None;
                app.request_redraw();
            }
        }
    }

    pub(super) fn update_worker_pending(&self) -> bool {
        self.updates
            .service
            .as_ref()
            .is_some_and(|service| !service.is_finished())
    }

    pub(super) fn update_committing(&self) -> bool {
        self.updates
            .attempt
            .as_ref()
            .is_some_and(|a| matches!(a.step, Step::Committing | Step::Exiting))
    }
}

#[cfg(test)]
mod tests;
