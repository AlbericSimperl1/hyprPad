// //! PipeWire stream consumer.
// //!
// //! Subscribes to the portal-provided node_id and receives video frames.
// //! Frames are pushed into an mpsc channel consumed by the encoder thread.
// //!
// //! Targets pipewire-rs 0.10 ("Box" API).

// use std::os::fd::OwnedFd;
// use std::sync::atomic::{AtomicBool, Ordering};
// use std::sync::mpsc::Sender;
// use std::sync::Arc;

// use pipewire as pw;
// use pw::{properties::properties, spa};

// use super::Frame;

// /// Shared state inside the stream listener.
// struct UserData {
//     tx: Sender<Frame>,
//     format: spa::param::video::VideoInfoRaw,
//     have_format: bool,
//     stop_flag: Arc<AtomicBool>,
//     /// Whether we already called quit to avoid double-quit.
//     quit_done: std::cell::Cell<bool>,
//     /// Raw pointer to the main loop, used to call quit.
//     mainloop_ptr: std::cell::Cell<*mut pw::sys::pw_main_loop>,
// }

// impl UserData {
//     /// Call quit on the main loop (once).
//     fn request_quit(&self) {
//         if self.quit_done.get() {
//             return;
//         }
//         self.quit_done.set(true);
//         unsafe {
//             let ptr = self.mainloop_ptr.get();
//             if !ptr.is_null() {
//                 pw::sys::pw_main_loop_quit(ptr);
//             }
//         }
//     }
// }

// /// Run the PipeWire main loop until `stop_flag` is set or the channel closes.
// /// `pw_fd` is consumed (PipeWire takes ownership).
// pub fn run_capture(
//     pw_fd: OwnedFd,
//     node_id: u32,
//     tx: Sender<Frame>,
//     stop_flag: Arc<AtomicBool>,
// ) -> Result<(), String> {
//     pw::init();

//     let mainloop =
//         pw::main_loop::MainLoopBox::new(None).map_err(|e| format!("MainLoopBox::new: {e}"))?;

//     // Store the raw pointer so UserData can call quit.
//     let mainloop_raw = mainloop.as_raw_ptr();

//     let context = pw::context::ContextBox::new(mainloop.loop_(), None)
//         .map_err(|e| format!("ContextBox::new: {e}"))?;
//     let core = context
//         .connect_fd(pw_fd, None)
//         .map_err(|e| format!("connect_fd: {e}"))?;

//     let data = UserData {
//         tx,
//         format: Default::default(),
//         have_format: false,
//         stop_flag: Arc::clone(&stop_flag),
//         quit_done: std::cell::Cell::new(false),
//         mainloop_ptr: std::cell::Cell::new(mainloop_raw),
//     };

//     let stream = pw::stream::StreamBox::new(
//         &core,
//         "hyprpad-capture",
//         properties! {
//             *pw::keys::MEDIA_TYPE => "Video",
//             *pw::keys::MEDIA_CATEGORY => "Capture",
//             *pw::keys::MEDIA_ROLE => "Screen",
//         },
//     )
//     .map_err(|e| format!("StreamBox::new: {e}"))?;

//     let _listener = stream
//         .add_local_listener_with_user_data(data)
//         .state_changed(|_, _, old, new| {
//             log::debug!("pw state: {:?} -> {:?}", old, new);
//         })
//         .param_changed(|_, user_data, id, param| {
//             let Some(param) = param else {
//                 return;
//             };
//             if id != pw::spa::param::ParamType::Format.as_raw() {
//                 return;
//             }

//             let (media_type, media_subtype) =
//                 match pw::spa::param::format_utils::parse_format(param) {
//                     Ok(v) => v,
//                     Err(_) => return,
//                 };

//             if media_type != pw::spa::param::format::MediaType::Video
//                 || media_subtype != pw::spa::param::format::MediaSubtype::Raw
//             {
//                 return;
//             }

//             user_data
//                 .format
//                 .parse(param)
//                 .expect("Failed to parse VideoInfoRaw");

//             user_data.have_format = true;
//             log::info!(
//                 "pw format negotiated: {:?} {}x{}",
//                 user_data.format.format(),
//                 user_data.format.size().width,
//                 user_data.format.size().height,
//             );
//         })
//         .process(|stream, user_data| {
//             // Check stop flag on every process callback.
//             if user_data.stop_flag.load(Ordering::SeqCst) {
//                 user_data.request_quit();
//                 return;
//             }

//             if !user_data.have_format {
//                 return;
//             }
//             match stream.dequeue_buffer() {
//                 None => {}
//                 Some(mut buffer) => {
//                     let datas = buffer.datas_mut();
//                     if datas.is_empty() {
//                         return;
//                     }

//                     let data = &mut datas[0];
//                     let chunk = data.chunk();
//                     let size = chunk.size() as usize;
//                     let stride = chunk.stride().max(0) as u32;

//                     if size == 0 {
//                         return;
//                     }

//                     let w = user_data.format.size().width;
//                     let h = user_data.format.size().height;
//                     let expected = (w as usize) * (h as usize) * 4; // BGR0 = 4 bytes/pixel
//                     let take = size.min(expected);

//                     if let Some(mapped) = data.data() {
//                         let slice = unsafe { std::slice::from_raw_parts(mapped.as_ptr(), take) };
//                         let frame = Frame {
//                             width: w,
//                             height: h,
//                             stride: if stride > 0 { stride } else { w * 4 },
//                             data: slice.to_vec(),
//                         };
//                         if user_data.tx.send(frame).is_err() {
//                             // Encoder gone — quit the main loop.
//                             user_data.request_quit();
//                         }
//                     }
//                 }
//             }
//         })
//         .register()
//         .map_err(|e| format!("listener register: {e}"))?;

//     // Build the format negotiation POD.
//     let obj = pw::spa::pod::object!(
//         pw::spa::utils::SpaTypes::ObjectParamFormat,
//         pw::spa::param::ParamType::EnumFormat,
//         pw::spa::pod::property!(
//             pw::spa::param::format::FormatProperties::MediaType,
//             Id,
//             pw::spa::param::format::MediaType::Video
//         ),
//         pw::spa::pod::property!(
//             pw::spa::param::format::FormatProperties::MediaSubtype,
//             Id,
//             pw::spa::param::format::MediaSubtype::Raw
//         ),
//         pw::spa::pod::property!(
//             pw::spa::param::format::FormatProperties::VideoFormat,
//             Choice,
//             Enum,
//             Id,
//             pw::spa::param::video::VideoFormat::BGRx,
//             pw::spa::param::video::VideoFormat::RGB,
//             pw::spa::param::video::VideoFormat::RGBA,
//             pw::spa::param::video::VideoFormat::RGBx,
//             pw::spa::param::video::VideoFormat::YUY2,
//         ),
//         pw::spa::pod::property!(
//             pw::spa::param::format::FormatProperties::VideoSize,
//             Choice,
//             Range,
//             Rectangle,
//             pw::spa::utils::Rectangle {
//                 width: 1920,
//                 height: 1080
//             },
//             pw::spa::utils::Rectangle {
//                 width: 1,
//                 height: 1
//             },
//             pw::spa::utils::Rectangle {
//                 width: 4096,
//                 height: 4096
//             }
//         ),
//         pw::spa::pod::property!(
//             pw::spa::param::format::FormatProperties::VideoFramerate,
//             Choice,
//             Range,
//             Fraction,
//             pw::spa::utils::Fraction { num: 30, denom: 1 },
//             pw::spa::utils::Fraction { num: 0, denom: 1 },
//             pw::spa::utils::Fraction { num: 120, denom: 1 }
//         ),
//     );

//     let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
//         std::io::Cursor::new(Vec::new()),
//         &pw::spa::pod::Value::Object(obj),
//     )
//     .unwrap()
//     .0
//     .into_inner();

//     let mut params = [spa::pod::Pod::from_bytes(&values).unwrap()];

//     stream
//         .connect(
//             spa::utils::Direction::Input,
//             Some(node_id),
//             pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
//             &mut params,
//         )
//         .map_err(|e| format!("stream.connect: {e}"))?;

//     log::info!("pw stream connected to node {node_id}");

//     // Periodically check the stop flag via an idle source that re-arms itself.
//     // This ensures the main loop breaks even when no frames arrive (e.g. the
//     // virtual monitor is idle / compositor not sending anything).
//     let stop_for_idle = stop_flag.clone();
//     mainloop.loop_().add_idle(true, move || {
//         if stop_for_idle.load(Ordering::SeqCst) {
//             unsafe {
//                 pw::sys::pw_main_loop_quit(mainloop_raw);
//             }
//         }
//     });

//     mainloop.run();

//     log::info!("pw main loop exited");
//     Ok(())
// }

// pub async fn open_screencast() -> Result<(OwnedFd, u32), String> {
//     Err("Screencast portal functie is tijdelijk uitgeschakeld.".to_string())
// }

//! xdg-desktop-portal screencast handshake.
//!
//! `open_screencast()` shows the portal popup (user picks a monitor), then
//! returns a PipeWire fd + the node_id to subscribe to. The fd is passed to
//! `pipewire::run_capture`.

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
