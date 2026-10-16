use crate::core::{Color, Rectangle, Size};
use crate::graphics::compositor::{self, Information};
use crate::graphics::damage;
use crate::graphics::error::{self, Error};
use crate::graphics::{self, Shell, Viewport};
use crate::{Layer, Renderer, Settings};

use std::collections::VecDeque;
use std::num::NonZeroU32;

pub struct Compositor {
    context: softbuffer::Context<Box<dyn compositor::Display>>,
    settings: Settings,
}

pub struct Surface {
    window: softbuffer::Surface<
        Box<dyn compositor::Display>,
        Box<dyn compositor::Window>,
    >,
    clip_mask: tiny_skia::Mask,
    layer_stack: VecDeque<Vec<Layer>>,
    background_color: Color,
    max_age: u8,
}

impl crate::graphics::Compositor for Compositor {
    type Renderer = Renderer;
    type Surface = Surface;

    async fn with_backend(
        settings: graphics::Settings,
        display: impl compositor::Display,
        _compatible_window: impl compositor::Window,
        _shell: Shell,
        backend: Option<&str>,
    ) -> Result<Self, Error> {
        match backend {
            None | Some("tiny-skia") | Some("tiny_skia") => {
                new(settings.into(), display)
            }
            Some(backend) => Err(Error::GraphicsAdapterNotFound {
                backend: "tiny-skia",
                reason: error::Reason::DidNotMatch {
                    preferred_backend: backend.to_owned(),
                },
            }),
        }
    }

    fn create_renderer(&self) -> Result<Self::Renderer, compositor::SurfaceError> {
        Ok(Renderer::new(
            self.settings.default_font,
            self.settings.default_text_size,
        ))
    }

    fn create_surface<W: compositor::Window + Clone>(
        &mut self,
        window: W,
        width: u32,
        height: u32,
    ) -> Result<Self::Surface, compositor::SurfaceError> {
        let window = softbuffer::Surface::new(
            &self.context,
            Box::new(window.clone()) as _,
        )
        .map_err(|error| compositor::SurfaceError::Creation(error.to_string()))?;

        let mut surface = Surface {
            window,
            clip_mask: tiny_skia::Mask::new(1, 1).ok_or(compositor::SurfaceError::OutOfMemory)?,
            layer_stack: VecDeque::new(),
            background_color: Color::TRANSPARENT,
            max_age: 0,
        };

        if width > 0 && height > 0 {
            self.configure_surface(&mut surface, width, height)?;
        }

        Ok(surface)
    }

    fn configure_surface(
        &mut self,
        surface: &mut Self::Surface,
        width: u32,
        height: u32,
    ) -> Result<(), compositor::SurfaceError> {
        let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else { return Ok(()) };
        surface
            .window
            .resize(
                width,
                height,
            )
            .map_err(|error| compositor::SurfaceError::Creation(error.to_string()))?;

        surface.clip_mask =
            tiny_skia::Mask::new(width.get(), height.get()).ok_or(compositor::SurfaceError::OutOfMemory)?;
        surface.layer_stack.clear();
        Ok(())
    }

    fn information(&self) -> Information {
        Information {
            adapter: String::from("CPU"),
            backend: String::from("tiny-skia"),
        }
    }

    fn present(
        &mut self,
        renderer: &mut Self::Renderer,
        surface: &mut Self::Surface,
        viewport: &Viewport,
        background_color: Color,
        on_pre_present: impl FnOnce(),
    ) -> Result<(), compositor::SurfaceError> {
        present(
            renderer,
            surface,
            viewport,
            background_color,
            on_pre_present,
        )
    }

    fn screenshot(
        &mut self,
        renderer: &mut Self::Renderer,
        viewport: &Viewport,
        background_color: Color,
    ) -> Result<Vec<u8>, compositor::SurfaceError> {
        screenshot(renderer, viewport, background_color)
    }
}

pub fn new(
    settings: Settings,
    display: impl compositor::Display,
) -> Result<Compositor, Error> {
    #[allow(unsafe_code)]
    let context = softbuffer::Context::new(Box::new(display) as _)
        .map_err(|error| Error::BackendError(error.to_string()))?;

    Ok(Compositor { context, settings })
}

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[test]
    fn unavailable_software_display_is_an_error() {
        struct UnavailableDisplay;
        impl raw_window_handle::HasDisplayHandle for UnavailableDisplay {
            fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
                Err(raw_window_handle::HandleError::Unavailable)
            }
        }
        assert!(new(Settings::default(), UnavailableDisplay).is_err());
    }

    #[test]
    fn empty_software_captures_are_refused_before_allocation() {
        let mut renderer = Renderer::new(Default::default(), 14.0.into());
        let viewport = Viewport::with_physical_size(Size::new(0, 100), 1.0);
        assert_eq!(screenshot(&mut renderer, &viewport, Color::TRANSPARENT), Err(compositor::SurfaceError::InvalidDimensions));
    }
}

pub fn present(
    renderer: &mut Renderer,
    surface: &mut Surface,
    viewport: &Viewport,
    background_color: Color,
    on_pre_present: impl FnOnce(),
) -> Result<(), compositor::SurfaceError> {
    let physical_size = viewport.physical_size();

    let mut buffer = surface
        .window
        .buffer_mut()
        .map_err(|_| compositor::SurfaceError::Lost)?;

    let last_layers = {
        let age = buffer.age();

        surface.max_age = surface.max_age.max(age);
        surface.layer_stack.truncate(surface.max_age as usize);

        if age > 0 {
            surface.layer_stack.get(age as usize - 1)
        } else {
            None
        }
    };

    let damage = last_layers
        .and_then(|last_layers| {
            (surface.background_color == background_color).then(|| {
                damage::diff(
                    last_layers,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    Layer::damage,
                )
            })
        })
        .unwrap_or_else(|| vec![Rectangle::with_size(viewport.logical_size())]);

    if damage.is_empty() {
        if let Some(last_layers) = last_layers {
            surface.layer_stack.push_front(last_layers.clone());
        }
    } else {
        surface.layer_stack.push_front(renderer.layers().to_vec());
        surface.background_color = background_color;

        let damage = damage::group(
            damage,
            Rectangle::with_size(viewport.logical_size()),
        );

        let mut pixels = tiny_skia::PixmapMut::from_bytes(
            bytemuck::cast_slice_mut(&mut buffer),
            physical_size.width,
            physical_size.height,
        )
        .ok_or(compositor::SurfaceError::InvalidDimensions)?;

        renderer.draw(
            &mut pixels,
            &mut surface.clip_mask,
            viewport,
            &damage,
            background_color,
        );
    }

    on_pre_present();
    buffer.present().map_err(|_| compositor::SurfaceError::Lost)
}

pub fn screenshot(
    renderer: &mut Renderer,
    viewport: &Viewport,
    background_color: Color,
) -> Result<Vec<u8>, compositor::SurfaceError> {
    let size = viewport.physical_size();

    if size.width == 0 || size.height == 0 { return Err(compositor::SurfaceError::InvalidDimensions) }
    let pixels = (size.width as usize).checked_mul(size.height as usize).ok_or(compositor::SurfaceError::InvalidDimensions)?;
    let mut offscreen_buffer: Vec<u32> = Vec::new();
    offscreen_buffer.try_reserve_exact(pixels).map_err(|_| compositor::SurfaceError::OutOfMemory)?;
    offscreen_buffer.resize(pixels, 0);

    let mut clip_mask = tiny_skia::Mask::new(size.width, size.height)
        .ok_or(compositor::SurfaceError::OutOfMemory)?;

    renderer.draw(
        &mut tiny_skia::PixmapMut::from_bytes(
            bytemuck::cast_slice_mut(&mut offscreen_buffer),
            size.width,
            size.height,
        )
        .ok_or(compositor::SurfaceError::InvalidDimensions)?,
        &mut clip_mask,
        viewport,
        &[Rectangle::with_size(Size::new(
            size.width as f32,
            size.height as f32,
        ))],
        background_color,
    );

    let bytes = pixels.checked_mul(4).ok_or(compositor::SurfaceError::InvalidDimensions)?;
    let mut output = Vec::new();
    output.try_reserve_exact(bytes).map_err(|_| compositor::SurfaceError::OutOfMemory)?;
    Ok(offscreen_buffer.iter().fold(
        output,
        |mut acc, pixel| {
            const A_MASK: u32 = 0xFF_00_00_00;
            const R_MASK: u32 = 0x00_FF_00_00;
            const G_MASK: u32 = 0x00_00_FF_00;
            const B_MASK: u32 = 0x00_00_00_FF;

            let a = ((A_MASK & pixel) >> 24) as u8;
            let r = ((R_MASK & pixel) >> 16) as u8;
            let g = ((G_MASK & pixel) >> 8) as u8;
            let b = (B_MASK & pixel) as u8;

            acc.extend([r, g, b, a]);
            acc
        },
    ))
}
