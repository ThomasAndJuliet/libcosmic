use sctk::data_device_manager::{
    data_device::DataDeviceHandler, data_offer::DataOfferHandler, data_source::DataSourceHandler,
};

use crate::platform_impl::wayland::state::WinitState;

impl DataDeviceHandler for WinitState {
    fn enter(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
        x: f64,
        y: f64,
        wl_surface: &wayland_client::protocol::wl_surface::WlSurface,
    ) {
        todo!()
    }

    fn leave(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
    ) {
        todo!()
    }

    fn motion(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
        x: f64,
        y: f64,
    ) {
        todo!()
    }

    fn selection(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
    ) {
        todo!()
    }

    fn drop_performed(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        data_device: &wayland_client::protocol::wl_data_device::WlDataDevice,
    ) {
        todo!()
    }
}

impl DataOfferHandler for WinitState {
    fn source_actions(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        offer: &mut sctk::data_device_manager::data_offer::DragOffer,
        actions: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
        todo!()
    }

    fn selected_action(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        offer: &mut sctk::data_device_manager::data_offer::DragOffer,
        actions: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
        todo!()
    }
}

impl DataSourceHandler for WinitState {
    fn accept_mime(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
        mime: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
        mime: String,
        fd: sctk::data_device_manager::WritePipe,
    ) {
    }

    fn cancelled(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
    ) {
    }

    fn dnd_dropped(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
    ) {
    }

    fn dnd_finished(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
    ) {
    }

    fn action(
        &mut self,
        conn: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
        source: &wayland_client::protocol::wl_data_source::WlDataSource,
        action: wayland_client::protocol::wl_data_device_manager::DndAction,
    ) {
    }
}

sctk::delegate_data_device!(WinitState);
