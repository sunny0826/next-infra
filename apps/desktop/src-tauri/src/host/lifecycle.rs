//! Platform-neutral Desktop Host lifecycle state machine.
//!
//! This module intentionally has no Tauri, AppKit, Runtime, or filesystem
//! dependency. The Tauri composition owner can translate the effects below to
//! platform calls while preserving the tested lifecycle semantics.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostState {
    NotRunning,
    StartingInteractive,
    StartingBackground,
    WindowVisible,
    WindowHidden,
    BackgroundOnly,
    GracefulExit(ShutdownPhase),
    UserQuitLatched,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownPhase {
    MarkerPending,
    Draining,
    Checkpointing,
    Stopping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchSource {
    UserInteractive,
    LoginAutostart,
    McpAuthorized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecondInstanceIntent {
    Interactive,
    LoginAutostartTuple,
    McpBackgroundTuple,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostEvent {
    Launch(LaunchSource),
    RuntimeReady,
    SecondInstance(SecondInstanceIntent),
    WindowCloseRequested,
    TrayRestore,
    DockReopen,
    ExplicitQuit,
    UserQuitMarkerDurable,
    UserQuitMarkerFailed,
    DrainCompleted,
    CheckpointCompleted,
    RuntimeStopped,
    RuntimeCrash,
    WebViewReload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostEffect {
    StartRuntimeInteractive,
    StartRuntimeBackground,
    CreateTray,
    CreateWindow,
    SetRegularActivation,
    PreventWindowClose,
    HideWindow,
    ShowWindow,
    UnminimizeWindow,
    FocusWindow,
    RequeryAfterRestore,
    RequeryAfterReload,
    ActivateExistingInstance,
    PersistUserQuit,
    ClearUserQuit,
    RejectMcpLaunch,
    DrainRuntime,
    CheckpointWal,
    StopRuntime,
    ExitProcess,
    CancelQuit,
    RecordCrash,
    IgnoreBackgroundSecondInstance,
    Ignored,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    pub from: HostState,
    pub to: HostState,
    pub effects: Vec<HostEffect>,
}

pub struct HostLifecycle {
    state: HostState,
    user_quit_latched: bool,
    pre_quit_state: Option<HostState>,
}

impl HostLifecycle {
    pub fn new(user_quit_latched: bool) -> Self {
        Self {
            state: if user_quit_latched {
                HostState::UserQuitLatched
            } else {
                HostState::NotRunning
            },
            user_quit_latched,
            pre_quit_state: None,
        }
    }

    pub fn state(&self) -> HostState {
        self.state
    }

    pub fn user_quit_latched(&self) -> bool {
        self.user_quit_latched
    }

    pub fn runtime_running(&self) -> bool {
        matches!(
            self.state,
            HostState::StartingInteractive
                | HostState::StartingBackground
                | HostState::WindowVisible
                | HostState::WindowHidden
                | HostState::BackgroundOnly
                | HostState::GracefulExit(_)
        )
    }

    pub fn dispatch(&mut self, event: HostEvent) -> Transition {
        let from = self.state;
        let mut effects = Vec::new();

        match (self.state, event) {
            (HostState::UserQuitLatched, HostEvent::Launch(LaunchSource::McpAuthorized)) => {
                effects.push(HostEffect::RejectMcpLaunch);
            }
            (HostState::NotRunning, HostEvent::Launch(source))
            | (HostState::UserQuitLatched, HostEvent::Launch(source)) => {
                self.launch(source, &mut effects);
            }
            (HostState::StartingInteractive, HostEvent::RuntimeReady) => {
                self.state = HostState::WindowVisible;
                effects.extend([
                    HostEffect::CreateTray,
                    HostEffect::CreateWindow,
                    HostEffect::SetRegularActivation,
                    HostEffect::FocusWindow,
                ]);
            }
            (HostState::StartingBackground, HostEvent::RuntimeReady) => {
                self.state = HostState::BackgroundOnly;
                effects.push(HostEffect::CreateTray);
            }
            (
                HostState::WindowVisible | HostState::WindowHidden | HostState::BackgroundOnly,
                HostEvent::SecondInstance(intent),
            ) => match intent {
                SecondInstanceIntent::LoginAutostartTuple
                | SecondInstanceIntent::McpBackgroundTuple => {
                    effects.push(HostEffect::IgnoreBackgroundSecondInstance);
                }
                SecondInstanceIntent::Interactive | SecondInstanceIntent::Unknown => {
                    self.restore_window(&mut effects);
                    effects.push(HostEffect::ActivateExistingInstance);
                }
            },
            (HostState::WindowVisible, HostEvent::WindowCloseRequested) => {
                self.state = HostState::WindowHidden;
                effects.extend([HostEffect::PreventWindowClose, HostEffect::HideWindow]);
            }
            (HostState::WindowHidden | HostState::BackgroundOnly, HostEvent::TrayRestore)
            | (HostState::WindowHidden | HostState::BackgroundOnly, HostEvent::DockReopen) => {
                self.restore_window(&mut effects);
            }
            (HostState::WindowVisible, HostEvent::TrayRestore | HostEvent::DockReopen) => {
                effects.extend([
                    HostEffect::SetRegularActivation,
                    HostEffect::FocusWindow,
                    HostEffect::RequeryAfterRestore,
                ]);
            }
            (
                HostState::WindowVisible | HostState::WindowHidden | HostState::BackgroundOnly,
                HostEvent::ExplicitQuit,
            ) => {
                self.pre_quit_state = Some(self.state);
                self.state = HostState::GracefulExit(ShutdownPhase::MarkerPending);
                effects.push(HostEffect::PersistUserQuit);
            }
            (
                HostState::GracefulExit(ShutdownPhase::MarkerPending),
                HostEvent::UserQuitMarkerDurable,
            ) => {
                self.user_quit_latched = true;
                self.pre_quit_state = None;
                self.state = HostState::GracefulExit(ShutdownPhase::Draining);
                effects.push(HostEffect::DrainRuntime);
            }
            (
                HostState::GracefulExit(ShutdownPhase::MarkerPending),
                HostEvent::UserQuitMarkerFailed,
            ) => {
                self.state = self
                    .pre_quit_state
                    .take()
                    .unwrap_or(HostState::WindowVisible);
                effects.push(HostEffect::CancelQuit);
            }
            (HostState::GracefulExit(ShutdownPhase::Draining), HostEvent::DrainCompleted) => {
                self.state = HostState::GracefulExit(ShutdownPhase::Checkpointing);
                effects.push(HostEffect::CheckpointWal);
            }
            (
                HostState::GracefulExit(ShutdownPhase::Checkpointing),
                HostEvent::CheckpointCompleted,
            ) => {
                self.state = HostState::GracefulExit(ShutdownPhase::Stopping);
                effects.push(HostEffect::StopRuntime);
            }
            (HostState::GracefulExit(ShutdownPhase::Stopping), HostEvent::RuntimeStopped) => {
                self.state = HostState::UserQuitLatched;
                effects.push(HostEffect::ExitProcess);
            }
            (
                HostState::StartingInteractive
                | HostState::StartingBackground
                | HostState::WindowVisible
                | HostState::WindowHidden
                | HostState::BackgroundOnly,
                HostEvent::RuntimeCrash,
            ) => {
                self.state = HostState::NotRunning;
                effects.push(HostEffect::RecordCrash);
            }
            (HostState::GracefulExit(phase), HostEvent::RuntimeCrash) => {
                self.state = if phase == ShutdownPhase::MarkerPending {
                    HostState::NotRunning
                } else {
                    HostState::UserQuitLatched
                };
                effects.push(HostEffect::RecordCrash);
            }
            (
                HostState::WindowVisible | HostState::WindowHidden | HostState::BackgroundOnly,
                HostEvent::WebViewReload,
            ) => {
                effects.push(HostEffect::RequeryAfterReload);
            }
            _ => effects.push(HostEffect::Ignored),
        }

        Transition {
            from,
            to: self.state,
            effects,
        }
    }

    fn launch(&mut self, source: LaunchSource, effects: &mut Vec<HostEffect>) {
        match source {
            LaunchSource::UserInteractive => {
                self.state = HostState::StartingInteractive;
                self.clear_marker_if_latched(effects);
                effects.push(HostEffect::StartRuntimeInteractive);
            }
            LaunchSource::LoginAutostart => {
                self.state = HostState::StartingBackground;
                self.clear_marker_if_latched(effects);
                effects.push(HostEffect::StartRuntimeBackground);
            }
            LaunchSource::McpAuthorized if self.user_quit_latched => {
                self.state = HostState::UserQuitLatched;
                effects.push(HostEffect::RejectMcpLaunch);
            }
            LaunchSource::McpAuthorized => {
                self.state = HostState::StartingBackground;
                effects.push(HostEffect::StartRuntimeBackground);
            }
        }
    }

    fn clear_marker_if_latched(&mut self, effects: &mut Vec<HostEffect>) {
        if self.user_quit_latched {
            self.user_quit_latched = false;
            effects.push(HostEffect::ClearUserQuit);
        }
    }

    fn restore_window(&mut self, effects: &mut Vec<HostEffect>) {
        let from = self.state;
        self.state = HostState::WindowVisible;
        effects.push(HostEffect::SetRegularActivation);
        match from {
            HostState::BackgroundOnly => effects.push(HostEffect::CreateWindow),
            HostState::WindowVisible => {}
            _ => effects.extend([HostEffect::ShowWindow, HostEffect::UnminimizeWindow]),
        }
        effects.extend([HostEffect::FocusWindow, HostEffect::RequeryAfterRestore]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(effects: &[HostEffect], effect: HostEffect) -> bool {
        effects.contains(&effect)
    }

    #[test]
    fn interactive_start_reaches_visible_window() {
        let mut lifecycle = HostLifecycle::new(false);
        let launch = lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        assert_eq!(launch.to, HostState::StartingInteractive);
        assert!(contains(
            &launch.effects,
            HostEffect::StartRuntimeInteractive
        ));

        let ready = lifecycle.dispatch(HostEvent::RuntimeReady);
        assert_eq!(ready.to, HostState::WindowVisible);
        assert!(contains(&ready.effects, HostEffect::CreateWindow));
        assert!(lifecycle.runtime_running());
    }

    #[test]
    fn autostart_and_mcp_start_background_without_creating_webview() {
        let mut autostart = HostLifecycle::new(false);
        assert_eq!(
            autostart
                .dispatch(HostEvent::Launch(LaunchSource::LoginAutostart))
                .to,
            HostState::StartingBackground
        );
        let ready = autostart.dispatch(HostEvent::RuntimeReady);
        assert_eq!(ready.to, HostState::BackgroundOnly);
        assert!(contains(&ready.effects, HostEffect::CreateTray));
        assert!(!contains(&ready.effects, HostEffect::CreateWindow));
        assert!(!contains(&ready.effects, HostEffect::SetRegularActivation));
        assert!(!contains(&ready.effects, HostEffect::FocusWindow));

        let mut mcp = HostLifecycle::new(false);
        mcp.dispatch(HostEvent::Launch(LaunchSource::McpAuthorized));
        assert_eq!(
            mcp.dispatch(HostEvent::RuntimeReady).to,
            HostState::BackgroundOnly
        );
    }

    #[test]
    fn close_hides_window_without_stopping_runtime_or_latching_quit() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        lifecycle.dispatch(HostEvent::RuntimeReady);

        let transition = lifecycle.dispatch(HostEvent::WindowCloseRequested);

        assert_eq!(transition.to, HostState::WindowHidden);
        assert!(contains(
            &transition.effects,
            HostEffect::PreventWindowClose
        ));
        assert!(contains(&transition.effects, HostEffect::HideWindow));
        assert!(lifecycle.runtime_running());
        assert!(!lifecycle.user_quit_latched());
    }

    #[test]
    fn tray_dock_and_interactive_second_instance_restore_and_requery() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        lifecycle.dispatch(HostEvent::RuntimeReady);
        lifecycle.dispatch(HostEvent::WindowCloseRequested);

        let tray = lifecycle.dispatch(HostEvent::TrayRestore);
        assert_eq!(tray.to, HostState::WindowVisible);
        assert!(contains(&tray.effects, HostEffect::RequeryAfterRestore));
        lifecycle.dispatch(HostEvent::WindowCloseRequested);

        let dock = lifecycle.dispatch(HostEvent::DockReopen);
        assert_eq!(dock.to, HostState::WindowVisible);
        lifecycle.dispatch(HostEvent::WindowCloseRequested);

        let second = lifecycle.dispatch(HostEvent::SecondInstance(SecondInstanceIntent::Unknown));
        assert_eq!(second.to, HostState::WindowVisible);
        assert!(contains(
            &second.effects,
            HostEffect::ActivateExistingInstance
        ));
    }

    #[test]
    fn background_second_instance_tuple_does_not_show_window() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::LoginAutostart));
        lifecycle.dispatch(HostEvent::RuntimeReady);

        let transition = lifecycle.dispatch(HostEvent::SecondInstance(
            SecondInstanceIntent::McpBackgroundTuple,
        ));

        assert_eq!(transition.to, HostState::BackgroundOnly);
        assert!(contains(
            &transition.effects,
            HostEffect::IgnoreBackgroundSecondInstance
        ));
        assert!(!contains(&transition.effects, HostEffect::ShowWindow));
    }

    #[test]
    fn background_restore_creates_window_and_requeries() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::LoginAutostart));
        lifecycle.dispatch(HostEvent::RuntimeReady);

        let transition = lifecycle.dispatch(HostEvent::TrayRestore);

        assert_eq!(transition.to, HostState::WindowVisible);
        assert!(contains(&transition.effects, HostEffect::CreateWindow));
        assert!(contains(
            &transition.effects,
            HostEffect::SetRegularActivation
        ));
        assert!(contains(&transition.effects, HostEffect::FocusWindow));
        assert!(contains(
            &transition.effects,
            HostEffect::RequeryAfterRestore
        ));
        assert!(!contains(&transition.effects, HostEffect::ShowWindow));
    }

    #[test]
    fn visible_restore_requeries_without_restarting_runtime() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        lifecycle.dispatch(HostEvent::RuntimeReady);

        let tray = lifecycle.dispatch(HostEvent::TrayRestore);
        assert_eq!(tray.to, HostState::WindowVisible);
        assert!(contains(&tray.effects, HostEffect::RequeryAfterRestore));
        assert!(!contains(&tray.effects, HostEffect::CreateWindow));
        assert!(!contains(
            &tray.effects,
            HostEffect::StartRuntimeInteractive
        ));

        let dock = lifecycle.dispatch(HostEvent::DockReopen);
        assert_eq!(dock.to, HostState::WindowVisible);
        assert!(contains(&dock.effects, HostEffect::RequeryAfterRestore));
        assert!(!contains(&dock.effects, HostEffect::CreateWindow));
    }

    #[test]
    fn visible_second_instance_only_activates_existing_window() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        lifecycle.dispatch(HostEvent::RuntimeReady);

        let transition =
            lifecycle.dispatch(HostEvent::SecondInstance(SecondInstanceIntent::Interactive));

        assert_eq!(transition.to, HostState::WindowVisible);
        assert!(contains(
            &transition.effects,
            HostEffect::ActivateExistingInstance
        ));
        assert!(contains(&transition.effects, HostEffect::FocusWindow));
        assert!(contains(
            &transition.effects,
            HostEffect::RequeryAfterRestore
        ));
        assert!(!contains(&transition.effects, HostEffect::CreateWindow));
        assert!(!contains(&transition.effects, HostEffect::ShowWindow));
        assert!(!contains(&transition.effects, HostEffect::UnminimizeWindow));
    }

    #[test]
    fn explicit_quit_persists_marker_drains_checkpoints_and_stops() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        lifecycle.dispatch(HostEvent::RuntimeReady);

        let request = lifecycle.dispatch(HostEvent::ExplicitQuit);
        assert_eq!(
            request.to,
            HostState::GracefulExit(ShutdownPhase::MarkerPending)
        );
        assert!(contains(&request.effects, HostEffect::PersistUserQuit));
        assert!(!lifecycle.user_quit_latched());

        let durable = lifecycle.dispatch(HostEvent::UserQuitMarkerDurable);
        assert_eq!(durable.to, HostState::GracefulExit(ShutdownPhase::Draining));
        assert!(contains(&durable.effects, HostEffect::DrainRuntime));
        let drained = lifecycle.dispatch(HostEvent::DrainCompleted);
        assert_eq!(
            drained.to,
            HostState::GracefulExit(ShutdownPhase::Checkpointing)
        );
        assert!(contains(&drained.effects, HostEffect::CheckpointWal));
        let checkpointed = lifecycle.dispatch(HostEvent::CheckpointCompleted);
        assert_eq!(
            checkpointed.to,
            HostState::GracefulExit(ShutdownPhase::Stopping)
        );
        assert!(contains(&checkpointed.effects, HostEffect::StopRuntime));
        let stopped = lifecycle.dispatch(HostEvent::RuntimeStopped);
        assert_eq!(stopped.to, HostState::UserQuitLatched);
        assert!(lifecycle.user_quit_latched());
        assert!(contains(&stopped.effects, HostEffect::ExitProcess));
    }

    #[test]
    fn marker_write_failure_cancels_quit_without_stopping_runtime() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        lifecycle.dispatch(HostEvent::RuntimeReady);
        lifecycle.dispatch(HostEvent::ExplicitQuit);

        let failed = lifecycle.dispatch(HostEvent::UserQuitMarkerFailed);
        assert_eq!(failed.to, HostState::WindowVisible);
        assert!(contains(&failed.effects, HostEffect::CancelQuit));
        assert!(lifecycle.runtime_running());
        assert!(!lifecycle.user_quit_latched());
    }

    #[test]
    fn crash_never_writes_user_quit_and_marker_durable_crash_keeps_latch() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        lifecycle.dispatch(HostEvent::RuntimeReady);
        let crashed = lifecycle.dispatch(HostEvent::RuntimeCrash);
        assert_eq!(crashed.to, HostState::NotRunning);
        assert!(contains(&crashed.effects, HostEffect::RecordCrash));
        assert!(!lifecycle.user_quit_latched());
        assert!(!contains(&crashed.effects, HostEffect::PersistUserQuit));

        let mut quitting = HostLifecycle::new(false);
        quitting.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        quitting.dispatch(HostEvent::RuntimeReady);
        quitting.dispatch(HostEvent::ExplicitQuit);
        quitting.dispatch(HostEvent::UserQuitMarkerDurable);
        let crash_after_marker = quitting.dispatch(HostEvent::RuntimeCrash);
        assert_eq!(crash_after_marker.to, HostState::UserQuitLatched);
        assert!(quitting.user_quit_latched());
    }

    #[test]
    fn webview_reload_requeries_without_restarting_runtime_or_marker() {
        let mut lifecycle = HostLifecycle::new(false);
        lifecycle.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        lifecycle.dispatch(HostEvent::RuntimeReady);

        let reload = lifecycle.dispatch(HostEvent::WebViewReload);

        assert_eq!(reload.to, HostState::WindowVisible);
        assert!(contains(&reload.effects, HostEffect::RequeryAfterReload));
        assert!(!contains(
            &reload.effects,
            HostEffect::StartRuntimeInteractive
        ));
        assert!(!contains(&reload.effects, HostEffect::StopRuntime));
        assert!(!lifecycle.user_quit_latched());
    }

    #[test]
    fn only_interactive_or_login_start_clears_user_quit_mcp_is_rejected() {
        let mut mcp = HostLifecycle::new(true);
        let rejected = mcp.dispatch(HostEvent::Launch(LaunchSource::McpAuthorized));
        assert_eq!(rejected.to, HostState::UserQuitLatched);
        assert!(contains(&rejected.effects, HostEffect::RejectMcpLaunch));
        assert!(mcp.user_quit_latched());

        let mut interactive = HostLifecycle::new(true);
        let launch = interactive.dispatch(HostEvent::Launch(LaunchSource::UserInteractive));
        assert_eq!(launch.to, HostState::StartingInteractive);
        assert!(contains(&launch.effects, HostEffect::ClearUserQuit));
        assert!(!interactive.user_quit_latched());

        let mut login = HostLifecycle::new(true);
        let launch = login.dispatch(HostEvent::Launch(LaunchSource::LoginAutostart));
        assert_eq!(launch.to, HostState::StartingBackground);
        assert!(contains(&launch.effects, HostEffect::ClearUserQuit));
        assert!(!login.user_quit_latched());
    }
}
