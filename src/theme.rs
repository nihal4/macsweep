//! Look & feel: fonts, colours, gradients and the custom widgets used everywhere.
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Pos2, Rect, Response, RichText, Sense, Shape, Stroke,
    Ui, Vec2, epaint::Mesh, vec2,
};
use std::f32::consts::TAU;
use std::sync::Arc;

/// Phosphor (fill) icon codepoints — font is bundled in assets/.
pub mod icon {
    pub const BROOM: &str = "\u{EC54}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const HARD_DRIVE: &str = "\u{E29E}";
    pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
    pub const PACKAGE: &str = "\u{E390}";
    pub const GEAR_SIX: &str = "\u{E272}";
    pub const SPARKLE: &str = "\u{E6A2}";
    pub const CHECK_CIRCLE: &str = "\u{E184}";
    pub const LOCK_SIMPLE: &str = "\u{E308}";
    pub const FILE_VIDEO: &str = "\u{EA22}";
    pub const FILE_ZIP: &str = "\u{E958}";
    pub const FILE: &str = "\u{E230}";
    pub const FILE_AUDIO: &str = "\u{EA20}";
    pub const FILE_IMAGE: &str = "\u{EA24}";
    pub const FILE_TEXT: &str = "\u{E23A}";
    pub const DISC: &str = "\u{E564}";
    pub const FOLDER: &str = "\u{E24A}";
    pub const WARNING: &str = "\u{E4E0}";
    pub const ARROW_CLOCKWISE: &str = "\u{E036}";
    pub const ROCKET_LAUNCH: &str = "\u{E3FE}";
    pub const CODE: &str = "\u{E1BC}";
    pub const BRAIN: &str = "\u{E74E}";
    pub const ENVELOPE: &str = "\u{E214}";
    pub const STOP: &str = "\u{E46C}";
    pub const TERMINAL: &str = "\u{E47E}";
    pub const GHOST: &str = "\u{E62A}";
}

/// Background gradient + accent for one section.
#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub top: Color32,
    pub bottom: Color32,
    pub accent: Color32,
}

pub const SMART: Palette = Palette {
    top: Color32::from_rgb(0x25, 0x12, 0x55),
    bottom: Color32::from_rgb(0x9a, 0x2a, 0x7c),
    accent: Color32::from_rgb(0xdf, 0x5b, 0xd4),
};
pub const JUNK: Palette = Palette {
    top: Color32::from_rgb(0x08, 0x36, 0x38),
    bottom: Color32::from_rgb(0x15, 0x8c, 0x66),
    accent: Color32::from_rgb(0x34, 0xd9, 0x9a),
};
pub const LARGE: Palette = Palette {
    top: Color32::from_rgb(0x45, 0x18, 0x10),
    bottom: Color32::from_rgb(0xbf, 0x55, 0x1c),
    accent: Color32::from_rgb(0xff, 0x9f, 0x45),
};
pub const APPS: Palette = Palette {
    top: Color32::from_rgb(0x0c, 0x1d, 0x52),
    bottom: Color32::from_rgb(0x2b, 0x6a, 0xcf),
    accent: Color32::from_rgb(0x5a, 0xa8, 0xff),
};
pub const AGENTS: Palette = Palette {
    top: Color32::from_rgb(0x0b, 0x2a, 0x3c),
    bottom: Color32::from_rgb(0x1a, 0x84, 0x96),
    accent: Color32::from_rgb(0x42, 0xd2, 0xe0),
};

pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x5f, 0x6d);
pub const WARN: Color32 = Color32::from_rgb(0xff, 0xb3, 0x40);

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn lerp_palette(a: Palette, b: Palette, t: f32) -> Palette {
    Palette {
        top: lerp(a.top, b.top, t),
        bottom: lerp(a.bottom, b.bottom, t),
        accent: lerp(a.accent, b.accent, t),
    }
}

fn lighten(c: Color32, t: f32) -> Color32 {
    lerp(c, Color32::WHITE, t)
}

// ---------- text ----------

fn fam(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

pub fn bold(s: impl Into<String>, size: f32) -> RichText {
    RichText::new(s.into()).size(size).family(fam("bold")).color(Color32::WHITE)
}
pub fn semi(s: impl Into<String>, size: f32) -> RichText {
    RichText::new(s.into())
        .size(size)
        .family(fam("semibold"))
        .color(Color32::WHITE)
}
pub fn text(s: impl Into<String>, size: f32) -> RichText {
    RichText::new(s.into()).size(size).color(Color32::WHITE)
}
pub fn dim(s: impl Into<String>, size: f32) -> RichText {
    RichText::new(s.into()).size(size).color(Color32::from_white_alpha(165))
}
pub fn ic(icon: &str, size: f32) -> RichText {
    RichText::new(icon).size(size).color(Color32::WHITE)
}
pub fn semi_font(size: f32) -> FontId {
    FontId::new(size, fam("semibold"))
}

// ---------- setup ----------

/// San Francisco (the system font) is memory-mapped, so it costs almost no RAM.
fn load_system_font() -> Option<egui::epaint::text::Blob> {
    let f = std::fs::File::open("/System/Library/Fonts/SFNS.ttf").ok()?;
    let map = unsafe { memmap2::Mmap::map(&f) }.ok()?;
    let blob: egui::epaint::text::Blob = Arc::new(map);
    // Only use it if it parses as the variable font we expect.
    if egui::FontData::from_blob(blob.clone(), 0).variation_axes().is_empty() {
        return None;
    }
    Some(blob)
}

pub fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "phosphor".into(),
        Arc::new(egui::FontData::from_static(include_bytes!("../assets/Phosphor-Fill.ttf"))),
    );
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let sf = load_system_font();
    for (name, weight) in [("regular", 400.0), ("semibold", 600.0), ("bold", 700.0)] {
        let mut chain = Vec::new();
        if let Some(blob) = &sf {
            let mut tweak = egui::epaint::text::FontTweak::default();
            tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", weight)]);
            let key = format!("sf-{name}");
            fonts
                .font_data
                .insert(key.clone(), Arc::new(egui::FontData::from_blob(blob.clone(), 0).tweak(tweak)));
            chain.push(key);
        }
        chain.extend(defaults.iter().cloned());
        chain.push("phosphor".into());
        if name == "regular" {
            fonts.families.insert(FontFamily::Proportional, chain.clone());
        }
        fonts.families.insert(fam(name), chain);
    }
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("phosphor".into());
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::ThemePreference::Dark);

    ctx.all_styles_mut(|s| {
        use egui::TextStyle::*;
        s.text_styles.insert(Body, FontId::proportional(14.0));
        s.text_styles.insert(Button, FontId::proportional(14.0));
        s.text_styles.insert(Small, FontId::proportional(11.5));
        s.text_styles.insert(Heading, FontId::new(24.0, fam("bold")));
        s.spacing.item_spacing = vec2(10.0, 8.0);
        s.spacing.button_padding = vec2(14.0, 6.0);
        s.spacing.interact_size.y = 26.0;

        let v = &mut s.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = Color32::TRANSPARENT;
        v.window_fill = Color32::from_rgb(0x24, 0x1a, 0x3a);
        v.extreme_bg_color = Color32::from_black_alpha(70);
        v.faint_bg_color = Color32::from_white_alpha(10);
        v.hyperlink_color = Color32::WHITE;
        v.selection.bg_fill = Color32::from_white_alpha(70);
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_white_alpha(235));
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(28));
        for (w, a) in [
            (&mut v.widgets.inactive, 30u8),
            (&mut v.widgets.hovered, 55),
            (&mut v.widgets.active, 75),
            (&mut v.widgets.open, 55),
        ] {
            w.corner_radius = CornerRadius::same(9);
            w.bg_fill = Color32::from_white_alpha(a);
            w.weak_bg_fill = Color32::from_white_alpha(a);
            w.bg_stroke = Stroke::NONE;
            w.fg_stroke = Stroke::new(1.5, Color32::WHITE);
            w.expansion = 0.0;
        }
    });
}

// ---------- painting ----------

/// Diagonal gradient filling `rect`, plus a soft light bloom in the top-right corner.
pub fn paint_background(painter: &egui::Painter, rect: Rect, p: Palette) {
    let mid = lerp(p.top, p.bottom, 0.5);
    let mut m = Mesh::default();
    m.colored_vertex(rect.left_top(), p.top);
    m.colored_vertex(rect.right_top(), mid);
    m.colored_vertex(rect.right_bottom(), p.bottom);
    m.colored_vertex(rect.left_bottom(), mid);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(m));
    radial_glow(
        painter,
        rect.right_top() + vec2(-120.0, 80.0),
        420.0,
        p.accent.gamma_multiply(0.22),
    );
}

/// A circle that fades from `color` in the middle to transparent at the edge.
pub fn radial_glow(painter: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    let n = 48;
    let mut m = Mesh::default();
    m.colored_vertex(c, color);
    for i in 0..=n {
        let a = i as f32 / n as f32 * TAU;
        m.colored_vertex(c + vec2(a.cos(), a.sin()) * r, Color32::TRANSPARENT);
    }
    for i in 1..=n as u32 {
        m.add_triangle(0, i, i + 1);
    }
    painter.add(Shape::mesh(m));
}

/// Disc with a vertical gradient (lighter on top), like macOS buttons.
pub fn gradient_disc(painter: &egui::Painter, c: Pos2, r: f32, top: Color32, bottom: Color32) {
    let n = 64;
    let mut m = Mesh::default();
    m.colored_vertex(c, lerp(top, bottom, 0.5));
    for i in 0..=n {
        let a = i as f32 / n as f32 * TAU;
        let p = c + vec2(a.cos(), a.sin()) * r;
        let t = ((p.y - (c.y - r)) / (2.0 * r)).clamp(0.0, 1.0);
        m.colored_vertex(p, lerp(top, bottom, t));
    }
    for i in 1..=n as u32 {
        m.add_triangle(0, i, i + 1);
    }
    painter.add(Shape::mesh(m));
}

pub fn arc(painter: &egui::Painter, c: Pos2, r: f32, from: f32, to: f32, stroke: Stroke) {
    let n = 64;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    painter.add(Shape::line(pts, stroke));
}

/// Big round icon at the top of every section. `progress` draws a ring (0..1),
/// `spinning` draws a rotating arc and keeps animating.
pub fn hero(ui: &mut Ui, glyph: &str, p: Palette, size: f32, spinning: bool, progress: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size + 44.0), Sense::hover());
    let c = rect.center();
    let r = size / 2.0;
    let painter = ui.painter();
    radial_glow(painter, c, r * 1.7, p.accent.gamma_multiply(0.45));
    gradient_disc(painter, c, r, lighten(p.accent, 0.35), lerp(p.accent, p.bottom, 0.35));
    painter.circle_stroke(c, r, Stroke::new(1.0, Color32::from_white_alpha(70)));
    painter.text(
        c,
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(size * 0.46),
        Color32::WHITE,
    );
    if let Some(f) = progress {
        painter.circle_stroke(c, r + 12.0, Stroke::new(6.0, Color32::from_white_alpha(35)));
        let start = -TAU / 4.0;
        arc(
            painter,
            c,
            r + 12.0,
            start,
            start + TAU * f.clamp(0.0, 1.0),
            Stroke::new(6.0, Color32::WHITE),
        );
    }
    if spinning {
        let t = ui.input(|i| i.time) as f32 * 3.2;
        arc(
            painter,
            c,
            r + 12.0,
            t,
            t + 1.6,
            Stroke::new(4.0, Color32::from_white_alpha(230)),
        );
        ui.ctx().request_repaint();
    }
}

/// CleanMyMac-style round action button.
pub fn round_button(ui: &mut Ui, label: &str, p: Palette, enabled: bool) -> Response {
    let d = 94.0;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(d + 22.0), sense);
    let c = rect.center();
    let hovered = enabled && resp.hovered();
    let pressed = enabled && resp.is_pointer_button_down_on();
    let painter = ui.painter();
    let fade = if enabled { 1.0 } else { 0.35 };
    if hovered {
        radial_glow(painter, c, d * 0.9, p.accent.gamma_multiply(0.5));
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    painter.circle_stroke(
        c,
        d / 2.0 + 7.0,
        Stroke::new(
            2.0,
            Color32::from_white_alpha(if hovered { 140 } else { 70 }).gamma_multiply(fade),
        ),
    );
    let r = if pressed { d / 2.0 - 2.0 } else { d / 2.0 };
    if enabled {
        gradient_disc(painter, c, r, lighten(p.accent, 0.3), lerp(p.accent, p.bottom, 0.2));
    } else {
        gradient_disc(painter, c, r, Color32::from_white_alpha(40), Color32::from_white_alpha(22));
    }
    painter.text(
        c,
        Align2::CENTER_CENTER,
        label,
        semi_font(17.0),
        Color32::WHITE.gamma_multiply(if enabled { 1.0 } else { 0.5 }),
    );
    resp
}

/// Rounded "pill" button.
pub fn pill(ui: &mut Ui, label: impl Into<String>, fill: Color32) -> Response {
    ui.add(
        egui::Button::new(semi(label, 13.5))
            .fill(fill)
            .corner_radius(16)
            .min_size(vec2(0.0, 32.0)),
    )
}

/// Translucent "glass" card.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(Color32::from_white_alpha(16))
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(26)))
        .corner_radius(16)
        .inner_margin(16)
        .show(ui, |ui| ui.vertical(add).inner)
}

/// Small rounded label, e.g. "Apple Silicon".
pub fn badge(ui: &mut Ui, label: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.75))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| ui.label(semi(label, 11.5)));
}

/// Coloured rounded square with an icon — used in lists.
pub fn icon_tile(ui: &mut Ui, glyph: &str, color: Color32, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let rr = rect.shrink(0.5);
    let r = (size * 0.26) as u8;
    painter.rect_filled(rr, r, color);
    // light sheen on the top half
    let top_half = Rect::from_min_max(rr.min, egui::pos2(rr.max.x, rr.center().y));
    painter.rect_filled(
        top_half,
        CornerRadius {
            nw: r,
            ne: r,
            sw: 0,
            se: 0,
        },
        Color32::from_white_alpha(22),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(size * 0.56),
        Color32::WHITE,
    );
    resp
}

/// Thin horizontal usage bar.
pub fn usage_bar(ui: &mut Ui, frac: f32, color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, height / 2.0, Color32::from_white_alpha(30));
    let mut fill = rect;
    fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
    painter.rect_filled(fill, height / 2.0, color);
}

/// Centered column helper.
pub fn centered(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.vertical_centered(add);
}
