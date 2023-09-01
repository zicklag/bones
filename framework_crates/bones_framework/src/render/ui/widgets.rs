//! Egui widgets.

mod bordered_button;
mod bordered_frame;

pub use bordered_button::*;
pub use bordered_frame::*;

use crate::prelude::*;

#[derive(HasSchema, Clone, Debug)]
#[repr(C)]
pub struct BorderImageMeta {
    pub image: Handle<Image>,
    pub image_size: UVec2,
    pub border_size: MarginMeta,
    pub scale: f32,
}

impl Default for BorderImageMeta {
    fn default() -> Self {
        Self {
            image: Default::default(),
            image_size: Default::default(),
            border_size: Default::default(),
            scale: 1.0,
        }
    }
}

#[derive(HasSchema, Clone, Debug, Default)]
#[repr(C)]
pub struct ButtonThemeMeta {
    pub font: FontMeta,
    pub padding: MarginMeta,
    pub borders: ButtonBordersMeta,
}

#[derive(HasSchema, Clone, Debug, Default)]
#[repr(C)]
pub struct ButtonBordersMeta {
    pub default: BorderImageMeta,
    pub focused: BorderImageMeta,
    pub clicked: BorderImageMeta,
}

#[derive(HasSchema, Default, serde::Deserialize, Clone, Copy, Debug)]
#[repr(C)]
pub struct MarginMeta {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl From<MarginMeta> for egui::style::Margin {
    fn from(m: MarginMeta) -> Self {
        Self {
            left: m.left,
            right: m.right,
            top: m.top,
            bottom: m.bottom,
        }
    }
}
