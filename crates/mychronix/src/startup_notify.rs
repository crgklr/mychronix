//! Tells the desktop Mychronix has started, so its launch cursor stops.
//!
//! On Wayland, a desktop like GNOME hands an app it launches a token
//! (`XDG_ACTIVATION_TOKEN`) and shows a busy cursor until the app's window
//! uses it. The windowing library can use it, but eframe doesn't pass it on,
//! so without this the cursor spins until the desktop gives up. Started from a
//! terminal there's no token and nothing to do.

/// Takes the launch token from the environment, so programs Mychronix starts
/// (nmcli and the like) don't inherit it. Call first thing, before any thread starts.
pub fn take_token() -> Option<String> {
    let token = std::env::var("XDG_ACTIVATION_TOKEN").ok().filter(|t| !t.is_empty());
    if token.is_some() {
        // SAFETY: called at the very start of main, before any other thread exists.
        unsafe { std::env::remove_var("XDG_ACTIVATION_TOKEN") };
    }
    token
}

/// Hands the token back for the new window (Wayland only; elsewhere it does nothing).
#[cfg(target_os = "linux")]
pub fn finish(token: Option<String>, window: &impl raw_window_handle::HasWindowHandle, display: &impl raw_window_handle::HasDisplayHandle) {
    let Some(token) = token else { return };
    if let Err(e) = wayland::activate(&token, window, display) {
        log::debug!("startup notification: {e}");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn finish<W, D>(_token: Option<String>, _window: &W, _display: &D) {}

#[cfg(target_os = "linux")]
mod wayland {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    use wayland_client::backend::{Backend, ObjectId};
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
    use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;

    struct State;

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
        fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
    }

    impl Dispatch<XdgActivationV1, ()> for State {
        fn event(_: &mut Self, _: &XdgActivationV1, _: <XdgActivationV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
    }

    pub fn activate(token: &str, window: &impl HasWindowHandle, display: &impl HasDisplayHandle) -> Result<(), String> {
        let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(window)) = (
            display.display_handle().map_err(|e| e.to_string())?.as_raw(),
            window.window_handle().map_err(|e| e.to_string())?.as_raw(),
        ) else {
            return Err("not on Wayland".into());
        };
        // SAFETY: the display and surface belong to the window eframe just made,
        // which outlives this call; we only add our own objects to the connection.
        let conn = Connection::from_backend(unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) });
        let surface_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), window.surface.as_ptr().cast()) }.map_err(|e| e.to_string())?;
        let surface = WlSurface::from_id(&conn, surface_id).map_err(|e| e.to_string())?;

        let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
        let activation: XdgActivationV1 = globals.bind(&queue.handle(), 1..=1, ()).map_err(|e| format!("no xdg_activation: {e}"))?;
        activation.activate(token.to_string(), &surface);
        activation.destroy();
        queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
        log::debug!("startup notification: done");
        Ok(())
    }
}
