use ashpd::desktop::{
    screencast::{
        CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream as ScreencastStream,
    },
    PersistMode,
};
use std::os::fd::OwnedFd;

/// Result of a successful portal handshake.
pub struct PortalHandle {
    pub fd: OwnedFd,
    pub node_id: u32,
}

/// Run the screencast portal flow. Triggers a system popup asking the user to
/// pick a monitor. Returns once they've approved.
pub async fn open_screencast() -> Result<PortalHandle, String> {
    let proxy = Screencast::new()
        .await
        .map_err(|e| format!("Screencast::new failed: {e}"))?;

    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|e| format!("create_session failed: {e}"))?;

    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Embedded)
                .set_sources(SourceType::Monitor | SourceType::Virtual)
                .set_multiple(false)
                .set_restore_token(None)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(|e| format!("select_sources failed: {e}"))?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(|e| format!("screencast start failed: {e}"))?
        .response()
        .map_err(|e| format!("screencast start response: {e}"))?;

    let stream: ScreencastStream = response
        .streams()
        .first()
        .ok_or_else(|| "Portal returned no streams".to_string())?
        .to_owned();

    let node_id = stream.pipe_wire_node_id();

    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .map_err(|e| format!("open_pipe_wire_remote failed: {e}"))?;

    Ok(PortalHandle { fd, node_id })
}
