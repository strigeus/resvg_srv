// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::cmp;
use std::rc::Rc;

use rgb::FromSlice;
use log::warn;
use usvg::ColorInterpolation as ColorSpace;

use crate::prelude::*;
use crate::filter::{self, Filter, ImageExt, Error, TransferFunctionExt};
use crate::ConvTransform;
use super::ReCairoContextExt;

type Image = filter::Image<cairo::ImageSurface>;
type FilterResult = filter::FilterResult<cairo::ImageSurface>;


pub fn apply(
    filter: &usvg::Filter,
    bbox: Option<Rect>,
    ts: &usvg::Transform,
    opt: &Options,
    background: Option<&cairo::ImageSurface>,
    canvas: &mut cairo::ImageSurface,
) {
    CairoFilter::apply(filter, bbox, ts, opt, background, canvas);
}


impl ImageExt for cairo::ImageSurface {
    fn width(&self) -> u32 {
        self.get_width() as u32
    }

    fn height(&self) -> u32 {
        self.get_height() as u32
    }

    fn try_clone(&self) -> Result<Self, Error> {
        let new_image = create_image(self.width(), self.height())?;

        let cr = cairo::Context::new(&new_image);
        cr.set_source_surface(self, 0.0, 0.0);
        cr.paint();

        Ok(new_image)
    }

    fn clip(&mut self, region: ScreenRect) {
        let cr = cairo::Context::new(self);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.set_operator(cairo::Operator::Clear);

        cr.rectangle(0.0, 0.0, self.width() as f64, region.y() as f64);
        cr.rectangle(0.0, 0.0, region.x() as f64, self.height() as f64);
        cr.rectangle(region.right() as f64, 0.0, self.width() as f64, self.height() as f64);
        cr.rectangle(0.0, region.bottom() as f64, self.width() as f64, self.height() as f64);

        cr.fill();
    }

    fn clear(&mut self) {
        let cr = cairo::Context::new(self);
        cr.set_operator(cairo::Operator::Clear);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.paint();
    }

    fn into_srgb(&mut self) {
        if let Ok(ref mut data) = self.get_data() {
            filter::from_premultiplied(data.as_bgra_mut());

            for p in data.as_bgra_mut() {
                p.r = filter::LINEAR_RGB_TO_SRGB_TABLE[p.r as usize];
                p.g = filter::LINEAR_RGB_TO_SRGB_TABLE[p.g as usize];
                p.b = filter::LINEAR_RGB_TO_SRGB_TABLE[p.b as usize];
            }

            filter::into_premultiplied(data.as_bgra_mut());
        } else {
            warn!("Cairo surface is already borrowed.");
        }
    }

    fn into_linear_rgb(&mut self) {
        if let Ok(ref mut data) = self.get_data() {
            filter::from_premultiplied(data.as_bgra_mut());

            for p in data.as_bgra_mut() {
                p.r = filter::SRGB_TO_LINEAR_RGB_TABLE[p.r as usize];
                p.g = filter::SRGB_TO_LINEAR_RGB_TABLE[p.g as usize];
                p.b = filter::SRGB_TO_LINEAR_RGB_TABLE[p.b as usize];
            }

            filter::into_premultiplied(data.as_bgra_mut());
        } else {
            warn!("Cairo surface is already borrowed.");
        }
    }
}

fn create_image(width: u32, height: u32) -> Result<cairo::ImageSurface, Error> {
    cairo::ImageSurface::create(cairo::Format::ARgb32, width as i32, height as i32)
        .map_err(|_| Error::AllocFailed)
}

fn copy_image(
    image: &cairo::ImageSurface,
    region: ScreenRect,
) -> Result<cairo::ImageSurface, Error> {
    let x = cmp::max(0, region.x()) as f64;
    let y = cmp::max(0, region.y()) as f64;

    let new_image = create_image(region.width(), region.height())?;

    let cr = cairo::Context::new(&new_image);
    cr.set_source_surface(&*image, -x, -y);
    cr.paint();

    Ok(new_image)
}

struct CairoFilter;

impl Filter<cairo::ImageSurface> for CairoFilter {
    fn get_input(
        input: &usvg::FilterInput,
        region: ScreenRect,
        results: &[FilterResult],
        background: Option<&cairo::ImageSurface>,
        canvas: &cairo::ImageSurface,
    ) -> Result<Image, Error> {
        match input {
            usvg::FilterInput::SourceGraphic => {
                let image = copy_image(canvas, region)?;

                Ok(Image {
                    image: Rc::new(image),
                    region: region.translate_to(0, 0),
                    color_space: ColorSpace::SRGB,
                })
            }
            usvg::FilterInput::SourceAlpha => {
                let mut image = copy_image(canvas, region)?;

                // Set RGB to black. Keep alpha as is.
                if let Ok(ref mut data) = image.get_data() {
                    for p in data.chunks_mut(4) {
                        p[0] = 0;
                        p[1] = 0;
                        p[2] = 0;
                    }
                } else {
                    warn!("Cairo surface is already borrowed.");
                }

                Ok(Image {
                    image: Rc::new(image),
                    region: region.translate_to(0, 0),
                    color_space: ColorSpace::SRGB,
                })
            }
            usvg::FilterInput::BackgroundImage => {
                let image = if let Some(image) = background {
                    copy_image(image, region)?
                } else {
                    create_image(canvas.width(), canvas.height())?
                };

                Ok(Image {
                    image: Rc::new(image),
                    region: region.translate_to(0, 0),
                    color_space: ColorSpace::SRGB,
                })
            }
            usvg::FilterInput::BackgroundAlpha => {
                let image = Self::get_input(
                    &usvg::FilterInput::BackgroundImage, region, results, background, canvas,
                )?;
                let mut image = image.take()?;

                // Set RGB to black. Keep alpha as is.
                if let Ok(ref mut data) = image.get_data() {
                    for p in data.chunks_mut(4) {
                        p[0] = 0;
                        p[1] = 0;
                        p[2] = 0;
                    }
                } else {
                    warn!("Cairo surface is already borrowed.");
                }

                Ok(Image {
                    image: Rc::new(image),
                    region: region.translate_to(0, 0),
                    color_space: ColorSpace::SRGB,
                })
            }
            usvg::FilterInput::Reference(ref name) => {
                if let Some(ref v) = results.iter().rev().find(|v| v.name == *name) {
                    Ok(v.image.clone())
                } else {
                    // Technically unreachable.
                    warn!("Unknown filter primitive reference '{}'.", name);
                    Self::get_input(
                        &usvg::FilterInput::SourceGraphic, region, results, background, canvas,
                    )
                }
            }
            _ => {
                warn!("Filter input '{:?}' is not supported.", input);
                Self::get_input(
                    &usvg::FilterInput::SourceGraphic, region, results, background, canvas,
                )
            }
        }
    }

    fn apply_blur(
        fe: &usvg::FeGaussianBlur,
        units: usvg::Units,
        cs: ColorSpace,
        bbox: Option<Rect>,
        ts: &usvg::Transform,
        input: Image,
    ) -> Result<Image, Error> {
        let (std_dx, std_dy, box_blur)
            = try_opt_or!(Self::resolve_std_dev(fe, units, bbox, ts), Ok(input));

        let input = input.into_color_space(cs)?;
        let mut buffer = input.take()?;

        let (w, h) = (buffer.width(), buffer.height());

        if let Ok(ref mut data) = buffer.get_data() {
            if box_blur {
                filter::box_blur::apply(data, w, h, std_dx, std_dy);
            } else {
                filter::iir_blur::apply(data, w, h, std_dx, std_dy);
            }
        }

        Ok(Image::from_image(buffer, cs))
    }

    fn apply_offset(
        fe: &usvg::FeOffset,
        units: usvg::Units,
        bbox: Option<Rect>,
        ts: &usvg::Transform,
        input: Image,
    ) -> Result<Image, Error> {
        let (dx, dy) = try_opt_or!(Self::resolve_offset(fe, units, bbox, ts), Ok(input));

        // TODO: do not use an additional buffer
        let buffer = create_image(input.width(), input.height())?;

        let cr = cairo::Context::new(&buffer);
        cr.set_source_surface(input.as_ref(), dx, dy);
        cr.paint();

        Ok(Image::from_image(buffer, input.color_space))
    }

    fn apply_blend(
        fe: &usvg::FeBlend,
        cs: ColorSpace,
        region: ScreenRect,
        input1: Image,
        input2: Image,
    ) -> Result<Image, Error> {
        let input1 = input1.into_color_space(cs)?;
        let input2 = input2.into_color_space(cs)?;

        let buffer = create_image(region.width(), region.height())?;
        let cr = cairo::Context::new(&buffer);

        cr.set_source_surface(input2.as_ref(), 0.0, 0.0);
        cr.paint();

        let operator = match fe.mode {
            usvg::FeBlendMode::Normal => cairo::Operator::Over,
            usvg::FeBlendMode::Multiply => cairo::Operator::Multiply,
            usvg::FeBlendMode::Screen => cairo::Operator::Screen,
            usvg::FeBlendMode::Darken => cairo::Operator::Darken,
            usvg::FeBlendMode::Lighten => cairo::Operator::Lighten,
        };

        cr.set_operator(operator);
        cr.set_source_surface(input1.as_ref(), 0.0, 0.0);
        cr.paint();

        Ok(Image::from_image(buffer, cs))
    }

    fn apply_composite(
        fe: &usvg::FeComposite,
        cs: ColorSpace,
        region: ScreenRect,
        input1: Image,
        input2: Image,
    ) -> Result<Image, Error> {
        use rgb::alt::BGRA8;

        let mut input1 = input1.into_color_space(cs)?.take()?;
        let mut input2 = input2.into_color_space(cs)?.take()?;

        let mut buffer = create_image(region.width(), region.height())?;

        if let Operator::Arithmetic { k1, k2, k3, k4 } = fe.operator {
            let data1 = input1.get_data().unwrap();
            let data2 = input2.get_data().unwrap();

            let calc = |i1, i2, max| {
                let i1 = i1 as f64 / 255.0;
                let i2 = i2 as f64 / 255.0;
                let result = k1.value() * i1 * i2 + k2.value() * i1 + k3.value() * i2 + k4.value();
                f64_bound(0.0, result, max)
            };

            {
                let mut i = 0;
                let mut data3 = buffer.get_data().unwrap();
                let data3 = data3.as_bgra_mut();
                for (c1, c2) in data1.as_bgra().iter().zip(data2.as_bgra()) {
                    let a = calc(c1.a, c2.a, 1.0);
                    if a.is_fuzzy_zero() {
                        i += 1;
                        continue;
                    }

                    let r = (calc(c1.r, c2.r, a) * 255.0) as u8;
                    let g = (calc(c1.g, c2.g, a) * 255.0) as u8;
                    let b = (calc(c1.b, c2.b, a) * 255.0) as u8;
                    let a = (a * 255.0) as u8;

                    data3[i] = BGRA8 { r, g, b, a };

                    i += 1;
                }
            }

            return Ok(Image::from_image(buffer, cs));
        }

        let cr = cairo::Context::new(&buffer);

        cr.set_source_surface(&input2, 0.0, 0.0);
        cr.paint();

        use usvg::FeCompositeOperator as Operator;
        let operator = match fe.operator {
            Operator::Over => cairo::Operator::Over,
            Operator::In => cairo::Operator::In,
            Operator::Out => cairo::Operator::Out,
            Operator::Atop => cairo::Operator::Atop,
            Operator::Xor => cairo::Operator::Xor,
            Operator::Arithmetic { .. } => cairo::Operator::Over,
        };

        cr.set_operator(operator);
        cr.set_source_surface(&input1, 0.0, 0.0);
        cr.paint();

        Ok(Image::from_image(buffer, cs))
    }

    fn apply_merge(
        fe: &usvg::FeMerge,
        cs: ColorSpace,
        region: ScreenRect,
        results: &[FilterResult],
        background: Option<&cairo::ImageSurface>,
        canvas: &cairo::ImageSurface,
    ) -> Result<Image, Error> {
        let buffer = create_image(region.width(), region.height())?;
        let cr = cairo::Context::new(&buffer);

        for input in &fe.inputs {
            let input = Self::get_input(input, region, &results, background, canvas)?;
            let input = input.into_color_space(cs)?;

            cr.set_source_surface(input.as_ref(), 0.0, 0.0);
            cr.paint();
        }

        Ok(Image::from_image(buffer, cs))
    }

    fn apply_flood(
        fe: &usvg::FeFlood,
        region: ScreenRect,
    ) -> Result<Image, Error> {
        let buffer = create_image(region.width(), region.height())?;

        let cr = cairo::Context::new(&buffer);
        cr.set_source_color(fe.color, fe.opacity);
        cr.paint();

        Ok(Image::from_image(buffer, ColorSpace::SRGB))
    }

    fn apply_tile(
        input: Image,
        region: ScreenRect,
    ) -> Result<Image, Error> {
        let buffer = create_image(region.width(), region.height())?;

        let subregion = input.region.translate(-region.x(), -region.y());

        let tile = copy_image(&input.image, subregion)?;
        let brush_ts = usvg::Transform::new_translate(subregion.x() as f64, subregion.y() as f64);

        let patt = cairo::SurfacePattern::create(&tile);
        patt.set_extend(cairo::Extend::Repeat);
        patt.set_filter(cairo::Filter::Best);

        let cr = cairo::Context::new(&buffer);
        let mut m: cairo::Matrix = brush_ts.to_native();
        m.invert();
        patt.set_matrix(m);

        cr.set_source(&patt);
        cr.rectangle(0.0, 0.0, region.width() as f64, region.height() as f64);
        cr.fill();

        Ok(Image::from_image(buffer, ColorSpace::SRGB))
    }

    fn apply_image(
        fe: &usvg::FeImage,
        region: ScreenRect,
        subregion: ScreenRect,
        opt: &Options,
    ) -> Result<Image, Error> {
        let buffer = create_image(region.width(), region.height())?;

        match fe.data {
            usvg::FeImageKind::None => {}
            usvg::FeImageKind::Image(ref data, format) => {
                let cr = cairo::Context::new(&buffer);

                let dx = (subregion.x() - region.x()) as f64;
                let dy = (subregion.y() - region.y()) as f64;
                cr.translate(dx, dy);

                let view_box = usvg::ViewBox {
                    rect: subregion.translate_to(0, 0).to_rect(),
                    aspect: fe.aspect,
                };

                if format == usvg::ImageFormat::SVG {
                    super::image::draw_svg(data, view_box, opt, &cr);
                } else {
                    super::image::draw_raster(format, data, view_box, fe.rendering_mode, opt, &cr);
                }
            }
            usvg::FeImageKind::Use(..) => {}
        }

        Ok(Image::from_image(buffer, ColorSpace::SRGB))
    }

    fn apply_component_transfer(
        fe: &usvg::FeComponentTransfer,
        cs: ColorSpace,
        input: Image,
    ) -> Result<Image, Error> {
        let input = input.into_color_space(cs)?;
        let mut buffer = input.take()?;

        if let Ok(ref mut data) = buffer.get_data() {
            filter::from_premultiplied(data.as_bgra_mut());

            for pixel in data.as_bgra_mut() {
                pixel.r = fe.func_r.apply(pixel.r);
                pixel.g = fe.func_g.apply(pixel.g);
                pixel.b = fe.func_b.apply(pixel.b);
                pixel.a = fe.func_a.apply(pixel.a);
            }

            filter::into_premultiplied(data.as_bgra_mut());
        }

        Ok(Image::from_image(buffer, cs))
    }

    fn apply_color_matrix(
        fe: &usvg::FeColorMatrix,
        cs: ColorSpace,
        input: Image,
    ) -> Result<Image, Error> {
        let input = input.into_color_space(cs)?;
        let mut buffer = input.take()?;

        if let Ok(ref mut data) = buffer.get_data() {
            filter::from_premultiplied(data.as_bgra_mut());
            filter::color_matrix::apply(&fe.kind, data.as_bgra_mut());
            filter::into_premultiplied(data.as_bgra_mut());
        }

        Ok(Image::from_image(buffer, cs))
    }

    fn apply_to_canvas(
        input: Image,
        region: ScreenRect,
        canvas: &mut cairo::ImageSurface,
    ) -> Result<(), Error> {
        let input = input.into_color_space(ColorSpace::SRGB)?;

        let cr = cairo::Context::new(canvas);

        cr.set_operator(cairo::Operator::Clear);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.paint();

        cr.set_operator(cairo::Operator::Over);
        cr.set_source_surface(input.as_ref(), region.x() as f64, region.y() as f64);
        cr.paint();

        Ok(())
    }
}
