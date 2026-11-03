//! Componentes compartilhados das telas (Tokens.dc.html), nos dois temas e só
//! com tokens de `theme.rs`. Todo componente interativo informa AccessKit
//! (papel, rótulo e estado), como os itens da lateral.
//!
//! # API
//!
//! Tipografia e espaço
//! - `Type::{Detail, Bar, Metric, Section, Body, Caption, Mono}.font() -> FontId`:
//!   18/600, 15/600, 24/600, 13/600, 13/400, 12/400 e 11/400 mono.
//! - `space::{XS, S, M, L, XL}`: 4, 8, 12, 16 e 24.
//! - `caption(ui, texto)`: legenda (12, text2), quebra linhas.
//! - Peso 500 do artboard: só há Inter Regular e SemiBold. Vira SemiBold onde
//!   marca estado (segmento selecionado) e Regular no resto (chip, botões).
//!
//! Superfícies
//! - `card(ui) -> egui::Frame`: cartão (surf, borda line, raio 10, sombra leve,
//!   margem 16 × 14). Uso: `card(ui).show(ui, |ui| …)`.
//! - `sheet(ui) -> egui::Frame`: folha (raio 12, sombra maior, margem 18 × 16).
//! - `card_grid(ui, &["Captura", …], |ui, i| …)`: um cartão por título, numa
//!   grade `repeat(auto-fit, minmax(260px, 1fr))` com gap 12. As colunas saem da
//!   largura disponível a cada frame (`grid_columns`); nada do layout anterior
//!   é guardado. Alturas iguais na linha. Cada cartão pinta o título (Seção) e
//!   é um nó AccessKit `Pane` com o título como rótulo.
//! - `grid_columns(largura, itens) -> usize`: a conta da grade acima.
//! - `toolbar(ui, |ui| esquerda, |ui| direita)`: linha de ferramentas no topo do
//!   conteúdo da página; a direita é montada da direita para a esquerda.
//!
//! Controles (todos respeitam `ui.add_enabled` / `add_enabled_ui`)
//! - `ui.add(Button::primary("Salvar"))`, `Button::secondary`,
//!   `Button::destructive`; `.small()` = 24 px e texto 12 (padrão: 28 px, 13).
//!   Raio 6. Desabilitado: `ui.add_enabled(false, Button::…)` pinta bg/line/text
//!   e o egui esmaece (≈ o cinza do artboard). AccessKit: `Button`, com
//!   `disabled` quando desabilitado.
//! - `segmented(ui, &mut atual, &[Segment { value, label, count }]) -> bool`
//!   (true = mudou). Fundo surf2, borda line, raio 7, itens de 24 px, texto 12;
//!   o selecionado tem surf, sombra e SemiBold; `count` vai em text2.
//!   AccessKit: um `Button` por item, rótulo "label count", marcado = selecionado.
//! - `switch(ui, &mut ligado, rótulo) -> Response`: interruptor 26 × 16 com
//!   rótulo; `changed()` no clique. AccessKit: `CheckBox` marcado/desmarcado.
//! - `disclosure_row(ui, id, título, meta, |ui| corpo) -> Option<R>`: linha de
//!   40 px com seta, título e `meta` (text2) à direita; `disclosure_link(ui, id,
//!   título, |ui| corpo) -> Option<R>`: seta e título em 12, cor acc. Recolhidos
//!   por padrão; o estado fica na memória do egui (`id` + id da `Ui`). Devolvem
//!   o retorno do corpo quando aberto. AccessKit: `Button` com `expanded`.
//!
//! Estado e medidas
//! - `chip(ui, Chip::{Ok, Late, Paused, Down}, texto) -> Response`: pílula de
//!   24 px, raio 12 (ok = sel/acc, atraso = warn translúcido/warn, pausado =
//!   surf2/text, indisponível = bad translúcido/bad). AccessKit: `Label`.
//! - `strip(ui, Tone::{Warn, Bad, Info}, negrito, texto, ação) -> Option<Response>`:
//!   faixa de aviso (fundo do tom a 8–10 %, ícone, trecho em negrito e texto).
//!   `ação = Some(rótulo)` põe um `Button::secondary(..).small()` à direita e
//!   devolve a resposta dele; o cursor segue abaixo da faixa inteira.
//!   AccessKit: `Label` "negrito texto".
//! - `meter(ui, fração, espessura, cor, rótulo) -> Response`: trilho line de
//!   `THIN` (4) ou `THICK` (8) px; `None` = desconhecido (só o trilho).
//!   AccessKit: `ProgressIndicator` com o valor em % (sem valor se `None`).
//! - `stacked_bar(ui, &[(valor, cor)], espessura, rótulo) -> Response`: partes
//!   proporcionais ao total; total 0 = só o trilho.
//! - `facts(ui, &[Fact::new(rótulo, valor).color(cor)])`: linhas rótulo (text2)
//!   … valor (SemiBold, `cor` ou text; `.mono()` = `Type::Mono`), espaçadas
//!   de 6. Numa linha só: o valor longo corta com reticências (o rótulo cede
//!   até 40 % da largura) e o texto cortado vai ao tooltip. AccessKit: um
//!   `Label` "rótulo: valor" por linha.
//! - `tone(cor, alfa) -> Color32`: o tom translúcido de chips e faixas.
//! - `strokes(painter, rect, &[&[(x, y), …]], largura, cor)` e `dot(painter,
//!   rect, (x, y), largura, cor)`: ícones a partir dos paths de 24 unidades dos
//!   artboards (um traço que volta ao ponto inicial é fechado).
use crate::theme;
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, FontId, Galley, Layout, Pos2, Rect, Response,
    Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use std::{hash::Hash, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Type {
    Detail,
    Bar,
    Metric,
    Section,
    Body,
    Caption,
    Mono,
}
impl Type {
    pub(crate) fn font(self) -> FontId {
        match self {
            Type::Detail => FontId::new(18., theme::semibold()),
            Type::Bar => FontId::new(15., theme::semibold()),
            Type::Metric => FontId::new(24., theme::semibold()),
            Type::Section => FontId::new(13., theme::semibold()),
            Type::Body => FontId::proportional(13.),
            Type::Caption => FontId::proportional(12.),
            Type::Mono => FontId::monospace(11.),
        }
    }
}

pub(crate) mod space {
    pub(crate) const XS: f32 = 4.;
    pub(crate) const S: f32 = 8.;
    pub(crate) const M: f32 = 12.;
    pub(crate) const L: f32 = 16.;
    pub(crate) const XL: f32 = 24.;
}

/// Espessuras do medidor e da barra empilhada.
pub(crate) const THIN: f32 = 4.;
pub(crate) const THICK: f32 = 8.;

/// Largura mínima de coluna e gap da grade de cartões.
const GRID_MIN: f32 = 260.;
const GRID_GAP: f32 = 12.;

/// `color` a `alpha` de opacidade (o Color32 é pré-multiplicado).
pub(crate) fn tone(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha)
}

fn shadow(p: &theme::Palette, blur: u8, y: i8) -> egui::Shadow {
    egui::Shadow {
        offset: [0, y],
        blur,
        spread: 0,
        color: p.shadow,
    }
}

pub(crate) fn card(ui: &Ui) -> egui::Frame {
    let p = theme::palette(ui.visuals());
    egui::Frame::new()
        .fill(p.surf)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(10.)
        .shadow(shadow(p, 2, 1))
        .inner_margin(egui::Margin::symmetric(16, 14))
}

#[allow(dead_code, reason = "API das passadas P2 e da Fase 4 (confirmações)")]
pub(crate) fn sheet(ui: &Ui) -> egui::Frame {
    let p = theme::palette(ui.visuals());
    egui::Frame::new()
        .fill(p.surf)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(12.)
        .shadow(shadow(p, 30, 10))
        .inner_margin(egui::Margin::symmetric(18, 16))
}

pub(crate) fn caption(ui: &mut Ui, text: &str) -> Response {
    let p = theme::palette(ui.visuals());
    ui.add(egui::Label::new(egui::RichText::new(text).size(12.).color(p.text2)).wrap())
}

/// Colunas de `repeat(auto-fit, minmax(260px, 1fr))` com gap 12: quantas
/// colunas de 260 cabem em `width`, ao menos 1 e no máximo uma por item
/// (auto-fit recolhe as faixas vazias).
pub(crate) fn grid_columns(width: f32, items: usize) -> usize {
    let fit = ((width + GRID_GAP) / (GRID_MIN + GRID_GAP)).floor();
    (fit.max(1.) as usize).min(items.max(1))
}

pub(crate) fn card_grid(ui: &mut Ui, titles: &[&str], mut add: impl FnMut(&mut Ui, usize)) {
    let width = ui.available_width();
    let columns = grid_columns(width, titles.len());
    let cell = (width - GRID_GAP * (columns - 1) as f32) / columns as f32;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = GRID_GAP;
        for start in (0..titles.len()).step_by(columns) {
            let top = ui.next_widget_position().y;
            let left = ui.max_rect().left();
            let mut cells = Vec::new();
            for (column, index) in (start..(start + columns).min(titles.len())).enumerate() {
                let rect = Rect::from_min_size(
                    pos2(left + column as f32 * (cell + GRID_GAP), top),
                    vec2(cell, f32::INFINITY),
                );
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .id_salt(("card", index))
                        .max_rect(rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                let mut prepared = card(&child).begin(&mut child);
                let content = &mut prepared.content_ui;
                content.set_min_width(content.available_width());
                content.label(egui::RichText::new(titles[index]).font(Type::Section.font()));
                add(content, index);
                cells.push((child, prepared, titles[index]));
            }
            let height = cells
                .iter()
                .map(|(_, prepared, _)| prepared.content_ui.min_rect().height())
                .fold(0., f32::max);
            let mut row = Rect::NOTHING;
            for (mut child, mut prepared, title) in cells {
                // A partir do topo do conteúdo (`set_min_height` conta do cursor).
                let top = prepared.content_ui.min_rect().top();
                prepared.content_ui.expand_to_include_y(top + height);
                let response = prepared.end(&mut child);
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Panel, true, title));
                row = row.union(response.rect);
            }
            ui.advance_cursor_after_rect(row);
        }
    });
}

pub(crate) fn toolbar<R>(
    ui: &mut Ui,
    left: impl FnOnce(&mut Ui),
    right: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.set_min_height(28.);
        ui.spacing_mut().item_spacing.x = space::S;
        left(ui);
        ui.with_layout(Layout::right_to_left(Align::Center), right)
            .inner
    })
    .inner
}

/// Traços num quadro de 24 unidades (os paths dos artboards), escalados a
/// `rect`; um traço que termina no ponto inicial é fechado.
pub(crate) fn strokes(
    painter: &egui::Painter,
    rect: Rect,
    lines: &[&[(f32, f32)]],
    width: f32,
    color: Color32,
) {
    let scale = rect.width() / 24.;
    let stroke = Stroke::new(width * scale, color);
    for points in lines {
        let points: Vec<Pos2> = points
            .iter()
            .map(|&(x, y)| rect.min + vec2(x, y) * scale)
            .collect();
        if points.len() > 2 && points.first() == points.last() {
            painter.add(egui::Shape::closed_line(points[1..].to_vec(), stroke));
        } else {
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}
/// Ponto (`h.01` dos paths) em coordenadas de 24 unidades.
pub(crate) fn dot(
    painter: &egui::Painter,
    rect: Rect,
    (x, y): (f32, f32),
    width: f32,
    color: Color32,
) {
    let scale = rect.width() / 24.;
    painter.circle_filled(rect.min + vec2(x, y) * scale, width * scale / 2., color);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Primary,
    Secondary,
    Destructive,
}
pub(crate) struct Button<'a> {
    text: &'a str,
    kind: Kind,
    small: bool,
}
impl<'a> Button<'a> {
    #[allow(dead_code, reason = "API das passadas P2 e da Fase 4")]
    pub(crate) fn primary(text: &'a str) -> Self {
        Self::new(text, Kind::Primary)
    }
    pub(crate) fn secondary(text: &'a str) -> Self {
        Self::new(text, Kind::Secondary)
    }
    #[allow(dead_code, reason = "API das passadas P2 e da Fase 4")]
    pub(crate) fn destructive(text: &'a str) -> Self {
        Self::new(text, Kind::Destructive)
    }
    fn new(text: &'a str, kind: Kind) -> Self {
        Self {
            text,
            kind,
            small: false,
        }
    }
    pub(crate) fn small(mut self) -> Self {
        self.small = true;
        self
    }
}
impl egui::Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = theme::palette(ui.visuals());
        let enabled = ui.is_enabled();
        let (fill, stroke, color) = match (enabled, self.kind) {
            (false, _) => (p.bg, Some(p.line), p.text),
            (true, Kind::Primary) => (p.acc, None, p.on_acc),
            (true, Kind::Secondary) => (p.surf, Some(p.line), p.text),
            (true, Kind::Destructive) => (p.surf, Some(p.line), p.bad),
        };
        let family = if self.kind == Kind::Primary {
            theme::semibold()
        } else {
            FontFamily::Proportional
        };
        let (size, height, pad) = match (self.small, self.kind) {
            (true, _) => (12., 24., 10.),
            (false, Kind::Primary) => (13., 28., 14.),
            (false, _) => (13., 28., 12.),
        };
        let galley =
            ui.painter()
                .layout_no_wrap(self.text.to_owned(), FontId::new(size, family), color);
        let (rect, response) =
            ui.allocate_exact_size(vec2(galley.size().x + 2. * pad, height), Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, self.text));
        let fill = if enabled && self.kind != Kind::Primary && response.hovered() {
            p.surf2
        } else {
            fill
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            6.,
            fill,
            stroke.map_or(Stroke::NONE, |c| Stroke::new(1.0_f32, c)),
            StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() / 2., galley, color);
        response
    }
}

pub(crate) struct Segment<'a, T> {
    pub value: T,
    pub label: &'a str,
    pub count: Option<String>,
}
pub(crate) fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    current: &mut T,
    items: &[Segment<'_, T>],
) -> bool {
    let p = theme::palette(ui.visuals());
    let mut changed = false;
    egui::Frame::new()
        .fill(p.surf2)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(7.)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.;
                for item in items {
                    let selected = *current == item.value;
                    let font = FontId::new(
                        12.,
                        if selected {
                            theme::semibold()
                        } else {
                            FontFamily::Proportional
                        },
                    );
                    let mut job = egui::text::LayoutJob::default();
                    let color = if selected { p.text } else { p.text2 };
                    job.append(item.label, 0., egui::TextFormat::simple(font, color));
                    let label = match &item.count {
                        Some(count) => {
                            job.append(
                                &format!(" {count}"),
                                0.,
                                egui::TextFormat::simple(FontId::proportional(12.), p.text2),
                            );
                            format!("{} {count}", item.label)
                        }
                        None => item.label.to_owned(),
                    };
                    let galley = ui.painter().layout_job(job);
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(galley.size().x + 24., 24.), Sense::click());
                    response.widget_info(|| {
                        WidgetInfo::selected(
                            WidgetType::SelectableLabel,
                            ui.is_enabled(),
                            selected,
                            &label,
                        )
                    });
                    if selected {
                        ui.painter().add(shadow(p, 2, 1).as_shape(rect, 5.));
                        ui.painter().rect_filled(rect, 5., p.surf);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 5., tone(p.surf, 0.5));
                    }
                    ui.painter()
                        .galley(rect.center() - galley.size() / 2., galley, color);
                    if response.clicked() && !selected {
                        *current = item.value;
                        changed = true;
                    }
                }
            });
        });
    changed
}

pub(crate) fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let p = theme::palette(ui.visuals());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), Type::Body.font(), p.text);
    let size = vec2(26. + 8. + galley.size().x, galley.size().y.max(16.));
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, label));
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - 8.), vec2(26., 16.));
    let painter = ui.painter();
    painter.rect_filled(track, 8., if *on { p.acc } else { p.line });
    // O botão claro do artboard: surf no tema claro (branco), text no escuro.
    let knob = if ui.visuals().dark_mode {
        p.text
    } else {
        p.surf
    };
    let x = if *on {
        track.left() + 12.
    } else {
        track.left() + 2.
    };
    painter.circle_filled(pos2(x + 6., track.center().y), 6., knob);
    painter.galley(
        pos2(track.right() + 8., rect.center().y - galley.size().y / 2.),
        galley,
        p.text,
    );
    response
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code, reason = "API das passadas P2 (barra superior e Docker)")]
pub(crate) enum Chip {
    Ok,
    Late,
    Paused,
    Down,
}
#[allow(dead_code, reason = "API das passadas P2 (barra superior e Docker)")]
pub(crate) fn chip(ui: &mut Ui, chip: Chip, text: &str) -> Response {
    let p = theme::palette(ui.visuals());
    let (fill, color) = match chip {
        Chip::Ok => (p.sel, p.acc),
        Chip::Late => (tone(p.warn, 0.12), p.warn),
        Chip::Paused => (p.surf2, p.text),
        Chip::Down => (tone(p.bad, 0.08), p.bad),
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), Type::Caption.font(), color);
    let mark = if chip == Chip::Paused { 10. } else { 7. };
    let (rect, response) = ui.allocate_exact_size(
        vec2(10. + mark + 6. + galley.size().x + 10., 24.),
        Sense::hover(),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    let painter = ui.painter();
    painter.rect_filled(rect, 12., fill);
    let mark = Rect::from_min_size(
        pos2(rect.left() + 10., rect.center().y - mark / 2.),
        Vec2::splat(mark),
    );
    if chip == Chip::Paused {
        strokes(
            painter,
            mark,
            &[&[(9., 5.), (9., 19.)], &[(15., 5.), (15., 19.)]],
            3.,
            color,
        );
    } else {
        painter.circle_filled(mark.center(), 3.5, color);
    }
    painter.galley(
        pos2(mark.right() + 6., rect.center().y - galley.size().y / 2.),
        galley,
        color,
    );
    response
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tone {
    Warn,
    Bad,
    Info,
}
impl Tone {
    fn color(self, p: &theme::Palette) -> Color32 {
        match self {
            Tone::Warn => p.warn,
            Tone::Bad => p.bad,
            Tone::Info => p.info,
        }
    }
    fn fill(self, p: &theme::Palette) -> Color32 {
        match self {
            Tone::Warn => tone(p.warn, 0.10),
            Tone::Bad => tone(p.bad, 0.08),
            Tone::Info => tone(p.info, 0.10),
        }
    }
}
pub(crate) fn strip(
    ui: &mut Ui,
    kind: Tone,
    bold: &str,
    text: &str,
    action: Option<&str>,
) -> Option<Response> {
    let p = theme::palette(ui.visuals());
    let color = kind.color(p);
    let width = ui.available_width();
    // Botão medido antes, para o texto quebrar no espaço que sobra.
    let button = action.map(|label| {
        let galley =
            ui.painter()
                .layout_no_wrap(label.to_owned(), FontId::proportional(12.), p.text);
        galley.size().x + 20.
    });
    let wrap = (width - 24. - 16. - 10. - button.map_or(0., |w| w + 10.)).max(40.);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    job.append(
        bold,
        0.,
        egui::TextFormat::simple(FontId::new(13., theme::semibold()), p.text),
    );
    if !text.is_empty() {
        job.append(
            &format!(" {text}"),
            0.,
            egui::TextFormat::simple(Type::Body.font(), p.text),
        );
    }
    let galley = ui.painter().layout_job(job);
    let height = galley.size().y.max(24.) + 20.;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let spoken = if text.is_empty() {
        bold.to_owned()
    } else {
        format!("{bold} {text}")
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &spoken));
    let painter = ui.painter();
    painter.rect_filled(rect, 8., kind.fill(p));
    let icon = Rect::from_min_size(
        pos2(rect.left() + 12., rect.center().y - 8.),
        Vec2::splat(16.),
    );
    match kind {
        Tone::Warn => {
            strokes(
                painter,
                icon,
                &[
                    &[(12., 3.), (21.5, 20.), (2.5, 20.), (12., 3.)],
                    &[(12., 10.), (12., 14.)],
                ],
                2.,
                color,
            );
            dot(painter, icon, (12., 17.), 2.4, color);
        }
        Tone::Bad => {
            painter.circle_stroke(
                icon.center(),
                9. * icon.width() / 24.,
                Stroke::new(2. * icon.width() / 24., color),
            );
            strokes(painter, icon, &[&[(12., 8.), (12., 13.)]], 2., color);
            dot(painter, icon, (12., 16.), 2.4, color);
        }
        Tone::Info => strokes(
            painter,
            icon,
            &[&[
                (4., 5.),
                (20., 5.),
                (20., 16.),
                (8., 16.),
                (4., 20.),
                (4., 5.),
            ]],
            2.,
            color,
        ),
    }
    painter.galley(
        pos2(icon.right() + 10., rect.center().y - galley.size().y / 2.),
        galley,
        p.text,
    );
    let width = button?;
    let at = Rect::from_min_size(
        pos2(rect.right() - 12. - width, rect.center().y - 12.),
        vec2(width, 24.),
    );
    // Num filho, que não mexe no cursor: `ui.put` o deixaria no fundo do
    // botão, dentro da faixa, e o próximo widget a sobreporia.
    let mut slot = ui.new_child(
        UiBuilder::new()
            .max_rect(at)
            .layout(Layout::centered_and_justified(egui::Direction::TopDown)),
    );
    Some(slot.add(Button::secondary(action.unwrap_or_default()).small()))
}

pub(crate) fn meter(
    ui: &mut Ui,
    fraction: Option<f32>,
    thickness: f32,
    color: Color32,
    label: &str,
) -> Response {
    let p = theme::palette(ui.visuals());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), thickness), Sense::hover());
    response.widget_info(|| WidgetInfo {
        value: fraction.map(|f| f64::from(f.clamp(0., 1.)) * 100.),
        ..WidgetInfo::labeled(WidgetType::ProgressIndicator, true, label)
    });
    let radius = thickness / 2.;
    ui.painter().rect_filled(rect, radius, p.line);
    if let Some(fraction) = fraction {
        let fill = Rect::from_min_size(
            rect.min,
            vec2(rect.width() * fraction.clamp(0., 1.), thickness),
        );
        if fill.width() > 0. {
            ui.painter().rect_filled(fill, radius, color);
        }
    }
    response
}

pub(crate) fn stacked_bar(
    ui: &mut Ui,
    parts: &[(f64, Color32)],
    thickness: f32,
    label: &str,
) -> Response {
    let p = theme::palette(ui.visuals());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), thickness), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ProgressIndicator, true, label));
    let radius = (thickness / 2.) as u8;
    ui.painter().rect_filled(rect, thickness / 2., p.line);
    let total: f64 = parts.iter().map(|&(v, _)| v.max(0.)).sum();
    if total <= 0. {
        return response;
    }
    let visible: Vec<_> = parts.iter().filter(|&&(v, _)| v > 0.).collect();
    let mut x = rect.left();
    for (i, &&(value, color)) in visible.iter().enumerate() {
        let width = rect.width() * (value / total) as f32;
        let corners = CornerRadius {
            nw: if i == 0 { radius } else { 0 },
            sw: if i == 0 { radius } else { 0 },
            ne: if i + 1 == visible.len() { radius } else { 0 },
            se: if i + 1 == visible.len() { radius } else { 0 },
        };
        ui.painter().rect_filled(
            Rect::from_min_size(pos2(x, rect.top()), vec2(width, thickness)),
            corners,
            color,
        );
        x += width;
    }
    response
}

pub(crate) struct Fact<'a> {
    label: &'a str,
    value: String,
    color: Option<Color32>,
    mono: bool,
}
impl<'a> Fact<'a> {
    pub(crate) fn new(label: &'a str, value: impl Into<String>) -> Self {
        Self {
            label,
            value: value.into(),
            color: None,
            mono: false,
        }
    }
    pub(crate) fn color(mut self, color: Option<Color32>) -> Self {
        self.color = color;
        self
    }
    /// Valor em `Type::Mono` (IDs, portas).
    pub(crate) fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}
/// Uma linha cortada numa largura (`truncate_at_width`).
fn single_line(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.));
    ui.painter().layout_job(job)
}
pub(crate) fn facts(ui: &mut Ui, rows: &[Fact<'_>]) {
    let p = theme::palette(ui.visuals());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 6.;
        for fact in rows {
            let value_color = fact.color.unwrap_or(p.text);
            let font = if fact.mono {
                Type::Mono.font()
            } else {
                FontId::new(13., theme::semibold())
            };
            let width = ui.available_width();
            // O valor corta onde começaria o rótulo inteiro, que cede só até
            // 40 % da linha; depois o rótulo corta no que o valor deixou.
            let label_width = ui
                .painter()
                .layout_no_wrap(fact.label.to_owned(), Type::Body.font(), p.text2)
                .size()
                .x;
            let value = single_line(
                ui,
                &fact.value,
                font,
                value_color,
                width - 12. - label_width.min(width * 0.4),
            );
            let label = single_line(
                ui,
                fact.label,
                Type::Body.font(),
                p.text2,
                width - value.size().x - 12.,
            );
            let (rect, response) = ui.allocate_exact_size(
                vec2(width, value.size().y.max(label.size().y)),
                Sense::hover(),
            );
            let spoken = format!("{}: {}", fact.label, fact.value);
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &spoken));
            let elided = (label.elided, value.elided);
            ui.painter().galley(
                pos2(rect.left(), rect.center().y - label.size().y / 2.),
                label,
                p.text2,
            );
            ui.painter().galley(
                pos2(
                    rect.right() - value.size().x,
                    rect.center().y - value.size().y / 2.,
                ),
                value,
                value_color,
            );
            match elided {
                (true, false) => {
                    response.on_hover_text(fact.label);
                }
                (false, true) => {
                    response.on_hover_text(&fact.value);
                }
                (true, true) => {
                    response.on_hover_text(&spoken);
                }
                (false, false) => {}
            }
        }
    });
}

/// Estado aberto de um recolhível, na memória do egui.
fn disclosure_state(ui: &Ui, id: egui::Id) -> bool {
    ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false)
}
fn chevron(painter: &egui::Painter, rect: Rect, open: bool, color: Color32) {
    let points: &[(f32, f32)] = if open {
        &[(6., 9.), (12., 15.), (18., 9.)]
    } else {
        &[(9., 6.), (15., 12.), (9., 18.)]
    };
    strokes(painter, rect, &[points], 2.5, color);
}
fn toggle(ui: &mut Ui, response: &Response, id: egui::Id, open: bool, title: &str) -> bool {
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), title));
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_expanded(open));
    if response.clicked() {
        ui.data_mut(|d| d.insert_temp(id, !open));
        return !open;
    }
    open
}

pub(crate) fn disclosure_row<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    title: &str,
    meta: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let p = theme::palette(ui.visuals());
    let id = ui.id().with(id_salt);
    let open = disclosure_state(ui, id);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.), Sense::click());
    let open = toggle(ui, &response, id, open, title);
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 0., tone(p.surf2, 0.6));
    }
    let arrow = Rect::from_min_size(
        pos2(rect.left() + 16., rect.center().y - 5.),
        Vec2::splat(10.),
    );
    chevron(painter, arrow, open, p.text);
    let meta = painter.layout_no_wrap(meta.to_owned(), Type::Caption.font(), p.text2);
    let meta_at = pos2(
        rect.right() - 16. - meta.size().x,
        rect.center().y - meta.size().y / 2.,
    );
    let mut job =
        egui::text::LayoutJob::simple_singleline(title.to_owned(), Type::Body.font(), p.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (meta_at.x - 12. - (arrow.right() + 8.)).max(0.),
    );
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(arrow.right() + 8., rect.center().y - galley.size().y / 2.),
        galley,
        p.text,
    );
    ui.painter().galley(meta_at, meta, p.text2);
    if !open {
        return None;
    }
    Some(
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                body(ui)
            })
            .inner,
    )
}

pub(crate) fn disclosure_link<R>(
    ui: &mut Ui,
    id_salt: impl Hash,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let p = theme::palette(ui.visuals());
    let id = ui.id().with(id_salt);
    let open = disclosure_state(ui, id);
    let galley = ui
        .painter()
        .layout_no_wrap(title.to_owned(), Type::Caption.font(), p.acc);
    let (rect, response) = ui.allocate_exact_size(
        vec2(10. + 6. + galley.size().x, galley.size().y.max(16.)),
        Sense::click(),
    );
    let open = toggle(ui, &response, id, open, title);
    let arrow = Rect::from_min_size(pos2(rect.left(), rect.center().y - 5.), Vec2::splat(10.));
    chevron(ui.painter(), arrow, open, p.acc);
    ui.painter().galley(
        pos2(arrow.right() + 6., rect.center().y - galley.size().y / 2.),
        galley,
        p.acc,
    );
    open.then(|| body(ui))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use egui::accesskit::{Node, Role, Toggled};

    pub(crate) const THEMES: [(egui::Theme, &theme::Palette); 2] = [
        (egui::Theme::Dark, &theme::DARK),
        (egui::Theme::Light, &theme::LIGHT),
    ];

    /// Contexto com as fontes e os estilos do app, no tema dado, com AccessKit.
    pub(crate) fn context(theme: egui::Theme) -> egui::Context {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.set_theme(theme);
        ctx.enable_accesskit();
        ctx
    }
    /// Um frame de 800 × 600 com `add` num painel central.
    pub(crate) fn run(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        add: impl FnMut(&mut Ui),
    ) -> egui::FullOutput {
        run_sized(ctx, [800., 600.], events, add)
    }
    pub(crate) fn run_sized(
        ctx: &egui::Context,
        size: [f32; 2],
        events: Vec<egui::Event>,
        mut add: impl FnMut(&mut Ui),
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size.into())),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| add(ui));
            },
        )
    }
    /// Nós do AccessKit com este papel e rótulo (ou valor, nos `Label`).
    pub(crate) fn nodes(out: &egui::FullOutput, role: Role, text: &str) -> Vec<Node> {
        out.platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.role() == role && (n.label() == Some(text) || n.value() == Some(text))
            })
            .map(|(_, n)| n.clone())
            .collect()
    }
    pub(crate) fn node(out: &egui::FullOutput, role: Role, text: &str) -> Node {
        match &nodes(out, role, text)[..] {
            [n] => n.clone(),
            found => panic!("expected one {role:?} {text:?}, found {}", found.len()),
        }
    }
    pub(crate) fn rect_of(node: &Node) -> Rect {
        let b = node.bounds().expect("node has bounds");
        Rect::from_min_max(
            pos2(b.x0 as f32, b.y0 as f32),
            pos2(b.x1 as f32, b.y1 as f32),
        )
    }
    /// Clique completo (pressiona e solta) em `pos`, em dois frames.
    pub(crate) fn click(ctx: &egui::Context, pos: Pos2, add: impl FnMut(&mut Ui)) {
        click_sized(ctx, [800., 600.], pos, add);
    }
    pub(crate) fn click_sized(
        ctx: &egui::Context,
        size: [f32; 2],
        pos: Pos2,
        mut add: impl FnMut(&mut Ui),
    ) {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        run_sized(
            ctx,
            size,
            vec![egui::Event::PointerMoved(pos), press(true)],
            &mut add,
        );
        run_sized(ctx, size, vec![press(false)], &mut add);
    }
    /// Todas as formas pintadas, com os `Shape::Vec` abertos.
    pub(crate) fn shapes(out: &egui::FullOutput) -> Vec<egui::Shape> {
        fn flat(shape: &egui::Shape, into: &mut Vec<egui::Shape>) {
            match shape {
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| flat(s, into)),
                other => into.push(other.clone()),
            }
        }
        let mut all = Vec::new();
        out.shapes.iter().for_each(|c| flat(&c.shape, &mut all));
        all
    }
    /// Retângulos pintados (com o preenchimento e o traço).
    pub(crate) fn rects(out: &egui::FullOutput) -> Vec<egui::epaint::RectShape> {
        shapes(out)
            .into_iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) => Some(r),
                _ => None,
            })
            .collect()
    }
    /// Textos pintados com a cor de cada um: a do primeiro trecho, ou a de
    /// reserva quando o trecho não tem cor própria.
    pub(crate) fn texts(out: &egui::FullOutput) -> Vec<(String, Color32)> {
        shapes(out)
            .into_iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) => {
                    let color = t
                        .override_text_color
                        .or(t
                            .galley
                            .job
                            .sections
                            .first()
                            .map(|s| s.format.color)
                            .filter(|&c| c != Color32::PLACEHOLDER))
                        .unwrap_or(t.fallback_color);
                    Some((t.galley.text().to_owned(), color))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn typography_matches_the_tokens() {
        use Type::*;
        for (t, size, family) in [
            (Detail, 18., theme::semibold()),
            (Bar, 15., theme::semibold()),
            (Metric, 24., theme::semibold()),
            (Section, 13., theme::semibold()),
            (Body, 13., FontFamily::Proportional),
            (Caption, 12., FontFamily::Proportional),
            (Mono, 11., FontFamily::Monospace),
        ] {
            // Um tipo novo tem de entrar nesta tabela.
            match t {
                Detail | Bar | Metric | Section | Body | Caption | Mono => {}
            }
            assert_eq!(t.font(), FontId::new(size, family), "{t:?}");
        }
        assert_eq!(
            [space::XS, space::S, space::M, space::L, space::XL],
            [4., 8., 12., 16., 24.]
        );
    }

    #[test]
    fn grid_columns_follow_auto_fit_minmax_260() {
        for (width, items, want) in [
            (0., 6, 1),
            (259., 6, 1),
            (531.9, 6, 1),
            // 2 × 260 + 12 de gap.
            (532., 6, 2),
            (594., 6, 2),
            (803.9, 6, 2),
            (804., 6, 3),
            (1040., 6, 3),
            (1620., 6, 6),
            // auto-fit: nunca mais colunas que itens.
            (5000., 6, 6),
            (1040., 2, 2),
            (1040., 1, 1),
            (1040., 0, 1),
        ] {
            assert_eq!(
                grid_columns(width, items),
                want,
                "{width} px, {items} itens"
            );
        }
    }

    #[test]
    fn card_and_sheet_use_the_tokens_in_both_themes() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let out = run(&ctx, vec![], |ui| {
                card(ui).show(ui, |ui| ui.label("cartão"));
                sheet(ui).show(ui, |ui| ui.label("folha"));
            });
            let frames: Vec<_> = rects(&out)
                .into_iter()
                .filter(|r| r.fill == p.surf && r.stroke == Stroke::new(1.0_f32, p.line))
                .map(|r| (r.corner_radius, r.blur_width))
                .collect();
            assert_eq!(
                frames,
                vec![(CornerRadius::same(10), 0.), (CornerRadius::same(12), 0.)],
                "{theme:?}"
            );
            let shadows: Vec<_> = rects(&out)
                .into_iter()
                .filter(|r| r.blur_width > 0.)
                .map(|r| (r.fill, r.blur_width))
                .collect();
            assert_eq!(shadows, vec![(p.shadow, 2.), (p.shadow, 30.)], "{theme:?}");
        }
    }

    #[test]
    fn button_kinds_paint_their_tokens_and_report_state() {
        for (theme, p) in THEMES {
            for (kind, fill, color) in [
                (Kind::Primary, p.acc, p.on_acc),
                (Kind::Secondary, p.surf, p.text),
                (Kind::Destructive, p.surf, p.bad),
            ] {
                let ctx = context(theme);
                let mut clicks = 0;
                let mut add = |ui: &mut Ui| {
                    if ui.add(Button::new("Ação", kind)).clicked() {
                        clicks += 1;
                    }
                    if ui.add_enabled(false, Button::new("Parada", kind)).clicked() {
                        clicks += 10;
                    }
                };
                let out = run(&ctx, vec![], &mut add);
                let ok = node(&out, Role::Button, "Ação");
                let off = node(&out, Role::Button, "Parada");
                assert!(!ok.is_disabled() && off.is_disabled(), "{theme:?} {kind:?}");
                assert_eq!(rect_of(&ok).height(), 28.);
                let painted = rects(&out);
                assert!(
                    painted
                        .iter()
                        .any(|r| r.fill == fill && r.corner_radius == CornerRadius::same(6)),
                    "{theme:?} {kind:?}: fill"
                );
                assert!(
                    texts(&out).contains(&("Ação".into(), color)),
                    "{theme:?} {kind:?}: text"
                );
                // Desabilitado: bg/line/text, esmaecido pelo egui (disabled_alpha).
                let alpha = ctx.style().visuals.disabled_alpha();
                let off_rect = rect_of(&off);
                assert_eq!(
                    painted
                        .iter()
                        .filter(|r| r.rect == off_rect)
                        .map(|r| r.fill)
                        .collect::<Vec<_>>(),
                    vec![p.bg.gamma_multiply(alpha)],
                    "{theme:?} {kind:?}: disabled fill"
                );
                let at = rect_of(&ok).center();
                let disabled = rect_of(&off).center();
                click(&ctx, at, &mut add);
                click(&ctx, disabled, &mut add);
                drop(add);
                assert_eq!(clicks, 1, "only the enabled button clicks");
            }
        }
        let ctx = context(egui::Theme::Light);
        let out = run(&ctx, vec![], |ui| {
            ui.add(Button::secondary("Pequeno").small());
        });
        assert_eq!(rect_of(&node(&out, Role::Button, "Pequeno")).height(), 24.);
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum View {
        A,
        B,
        C,
    }

    #[test]
    fn segmented_selects_one_item_and_reports_counts() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let mut current = View::A;
            let mut changes = 0;
            let mut add = |ui: &mut Ui| {
                let items = [
                    Segment {
                        value: View::A,
                        label: "Handoffs",
                        count: Some("75".into()),
                    },
                    Segment {
                        value: View::B,
                        label: "Sessões",
                        count: None,
                    },
                    Segment {
                        value: View::C,
                        label: "Projetos",
                        count: Some("0".into()),
                    },
                ];
                if segmented(ui, &mut current, &items) {
                    changes += 1;
                }
            };
            let out = run(&ctx, vec![], &mut add);
            let toggled = |out: &egui::FullOutput, text| node(out, Role::Button, text).toggled();
            assert_eq!(
                toggled(&out, "Handoffs 75"),
                Some(Toggled::True),
                "{theme:?}"
            );
            assert_eq!(toggled(&out, "Sessões"), Some(Toggled::False));
            assert_eq!(toggled(&out, "Projetos 0"), Some(Toggled::False));
            assert_eq!(rect_of(&node(&out, Role::Button, "Sessões")).height(), 24.);
            assert!(
                rects(&out)
                    .iter()
                    .any(|r| r.fill == p.surf2 && r.corner_radius == CornerRadius::same(7)),
                "{theme:?}: track"
            );
            let at = rect_of(&node(&out, Role::Button, "Sessões")).center();
            click(&ctx, at, &mut add);
            let out = run(&ctx, vec![], &mut add);
            assert_eq!(toggled(&out, "Sessões"), Some(Toggled::True));
            assert_eq!(toggled(&out, "Handoffs 75"), Some(Toggled::False));
            // Clicar no já selecionado não conta como mudança.
            click(&ctx, at, &mut add);
            drop(add);
            assert_eq!((current, changes), (View::B, 1), "{theme:?}");
        }
    }

    #[test]
    fn switch_toggles_and_reports_checked_state() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let mut on = false;
            let mut changed = 0;
            let mut add = |ui: &mut Ui| {
                if switch(ui, &mut on, "Só em execução").changed() {
                    changed += 1;
                }
            };
            let out = run(&ctx, vec![], &mut add);
            let state =
                |out: &egui::FullOutput| node(out, Role::CheckBox, "Só em execução").toggled();
            assert_eq!(state(&out), Some(Toggled::False), "{theme:?}");
            let at = rect_of(&node(&out, Role::CheckBox, "Só em execução")).center();
            click(&ctx, at, &mut add);
            let out = run(&ctx, vec![], &mut add);
            assert_eq!(state(&out), Some(Toggled::True));
            assert!(
                rects(&out)
                    .iter()
                    .any(|r| r.fill == p.acc && r.rect.size() == vec2(26., 16.)),
                "{theme:?}: the on track is acc"
            );
            click(&ctx, at, &mut add);
            drop(add);
            assert_eq!((on, changed), (false, 2));
        }
    }

    #[test]
    fn chip_tones_cover_every_state_in_both_themes() {
        for (theme, p) in THEMES {
            for chip_kind in [Chip::Ok, Chip::Late, Chip::Paused, Chip::Down] {
                let (fill, color) = match chip_kind {
                    Chip::Ok => (p.sel, p.acc),
                    Chip::Late => (p.warn.gamma_multiply(0.12), p.warn),
                    Chip::Paused => (p.surf2, p.text),
                    Chip::Down => (p.bad.gamma_multiply(0.08), p.bad),
                };
                let ctx = context(theme);
                let out = run(&ctx, vec![], |ui| {
                    chip(ui, chip_kind, "Estado");
                });
                let r = rect_of(&node(&out, Role::Label, "Estado"));
                assert_eq!(r.height(), 24., "{theme:?} {chip_kind:?}");
                assert!(
                    rects(&out)
                        .iter()
                        .any(|s| s.fill == fill && s.corner_radius == CornerRadius::same(12)),
                    "{theme:?} {chip_kind:?}: fill"
                );
                assert!(
                    texts(&out).contains(&("Estado".into(), color)),
                    "{theme:?} {chip_kind:?}"
                );
            }
        }
    }

    #[test]
    fn strip_tones_text_and_optional_action() {
        for (theme, p) in THEMES {
            for (kind, fill) in [
                (Tone::Warn, p.warn.gamma_multiply(0.10)),
                (Tone::Bad, p.bad.gamma_multiply(0.08)),
                (Tone::Info, p.info.gamma_multiply(0.10)),
            ] {
                let ctx = context(theme);
                let mut pressed = 0;
                let mut add = |ui: &mut Ui| {
                    strip(
                        ui,
                        kind,
                        "Dados parciais.",
                        "2 projetos não responderam.",
                        None,
                    );
                    if strip(ui, kind, "Sem conexão.", "", Some("Abrir Conexão"))
                        .is_some_and(|r| r.clicked())
                    {
                        pressed += 1;
                    }
                };
                let out = run(&ctx, vec![], &mut add);
                node(
                    &out,
                    Role::Label,
                    "Dados parciais. 2 projetos não responderam.",
                );
                node(&out, Role::Label, "Sem conexão.");
                assert_eq!(
                    rects(&out).iter().filter(|r| r.fill == fill).count(),
                    2,
                    "{theme:?} {kind:?}: both strips use the tone"
                );
                let button = rect_of(&node(&out, Role::Button, "Abrir Conexão"));
                let strip = rect_of(&node(&out, Role::Label, "Sem conexão."));
                assert!(
                    strip.contains_rect(button),
                    "the action sits inside its strip"
                );
                click(&ctx, button.center(), &mut add);
                drop(add);
                assert_eq!(pressed, 1, "{theme:?} {kind:?}");
            }
        }
    }

    #[test]
    fn strip_text_wraps_inside_a_narrow_column() {
        let ctx = context(egui::Theme::Dark);
        let long = "palavra ".repeat(40);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300., 600.))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    strip(ui, Tone::Warn, "Aviso.", long.trim(), Some("Detalhes"));
                });
            },
        );
        let strip = rect_of(&node(&out, Role::Label, &format!("Aviso. {}", long.trim())));
        assert!(strip.right() <= 300., "{strip:?}");
        assert!(
            strip.height() > 60.,
            "long text must wrap, not overflow: {strip:?}"
        );
    }

    #[test]
    fn the_widget_after_a_strip_with_an_action_starts_below_the_whole_strip() {
        let long = "palavra ".repeat(40);
        // (largura, texto, linhas mínimas): 1 linha e 3+ linhas.
        for (width, text, lines) in [(800., "Texto curto.", 1), (300., long.trim(), 3)] {
            for (theme, _) in THEMES {
                let ctx = context(theme);
                let out = run_sized(&ctx, [width, 600.], vec![], |ui| {
                    strip(ui, Tone::Warn, "Aviso.", text, Some("Abrir"));
                    ui.label("Depois da faixa");
                });
                let strip = rect_of(&node(&out, Role::Label, &format!("Aviso. {text}")));
                let button = rect_of(&node(&out, Role::Button, "Abrir"));
                let after = rect_of(&node(&out, Role::Label, "Depois da faixa"));
                assert!(
                    strip.height() >= lines as f32 * 15. + 20.,
                    "{width}: precondition, {lines}+ lines: {strip:?}"
                );
                assert!(strip.contains_rect(button), "{button:?} in {strip:?}");
                let gap = ctx.style().spacing.item_spacing.y;
                assert_eq!(
                    after.top(),
                    strip.bottom() + gap,
                    "{theme:?} {width}: the next widget follows the strip, not its button"
                );
            }
        }
    }

    #[test]
    fn long_fact_values_are_cut_beside_their_label_and_mono_uses_the_mono_font() {
        let image = "ghcr.io/organizacao/uma-imagem-com-um-nome-muito-comprido:2026.11.03";
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let mut add = |ui: &mut Ui| {
                ui.allocate_ui(vec2(280., 400.), |ui| {
                    facts(
                        ui,
                        &[
                            Fact::new("Imagem", image),
                            Fact::new("ID", "8f3c21a9d0e4").mono(),
                            Fact::new("Curto", "ok"),
                        ],
                    );
                });
            };
            let out = run(&ctx, vec![], &mut add);
            let row = rect_of(&node(&out, Role::Label, &format!("Imagem: {image}")));
            let painted = |text: &str| {
                out.shapes
                    .iter()
                    .find_map(|c| match &c.shape {
                        egui::Shape::Text(t) if t.galley.text().starts_with(text) => {
                            Some(t.clone())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("{text} painted"))
            };
            let label = painted("Imagem");
            let value = painted("ghcr.io");
            assert!(value.galley.elided, "{theme:?}: the long value is cut");
            assert!(!label.galley.elided, "{theme:?}: the short label is whole");
            let logical = |t: &egui::epaint::TextShape| t.galley.rect.translate(t.pos.to_vec2());
            let (label, value) = (logical(&label), logical(&value));
            assert!(
                value.left() >= label.right() + 12. - 0.5 && value.right() <= row.right() + 0.5,
                "{theme:?}: {value:?} overlaps {label:?} or leaves {row:?}"
            );
            assert_eq!(
                painted("8f3c21a9d0e4").galley.job.sections[0]
                    .format
                    .font_id,
                Type::Mono.font()
            );
            assert_eq!(
                painted("ok").galley.job.sections[0].format.font_id,
                FontId::new(13., theme::semibold())
            );
            assert!(texts(&out).contains(&("ok".into(), p.text)));
            // O valor cortado inteiro no tooltip.
            let hover = egui::Event::PointerMoved(row.center());
            let mut out = run(&ctx, vec![hover], &mut add);
            for _ in 0..45 {
                out = run(&ctx, vec![], &mut add);
            }
            node(&out, Role::Label, image);
        }
    }

    #[test]
    fn meter_and_stacked_bar_fill_by_value_over_a_line_track() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let out = run(&ctx, vec![], |ui| {
                ui.allocate_ui(vec2(200., 400.), |ui| {
                    meter(ui, Some(0.25), THICK, p.acc, "Uso");
                    meter(ui, None, THIN, p.acc, "Desconhecido");
                    meter(ui, Some(7.), THIN, p.warn, "Acima");
                    stacked_bar(
                        ui,
                        &[(3., p.acc), (0., p.warn), (1., p.bad)],
                        THICK,
                        "Partes",
                    );
                    stacked_bar(ui, &[(0., p.acc)], THICK, "Vazia");
                });
            });
            let value = |text| node(&out, Role::ProgressIndicator, text).numeric_value();
            assert_eq!(value("Uso"), Some(25.));
            assert_eq!(value("Desconhecido"), None, "unknown is not 0");
            assert_eq!(value("Acima"), Some(100.), "clamped");
            let shapes = rects(&out);
            let width_of = |color: Color32, height: f32| {
                shapes
                    .iter()
                    .filter(|r| r.fill == color && r.rect.height() == height)
                    .map(|r| r.rect.width())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                width_of(p.acc, THICK),
                vec![50., 150.],
                "{theme:?}: 25% meter, 3/4 bar"
            );
            assert_eq!(width_of(p.bad, THICK), vec![50.]);
            assert_eq!(width_of(p.warn, THIN), vec![200.]);
            assert_eq!(
                width_of(p.line, THICK).len(),
                3,
                "every bar draws its track"
            );
            assert_eq!(width_of(p.line, THIN).len(), 2);
        }
    }

    #[test]
    fn facts_speak_label_and_value_and_keep_the_dash() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let out = run(&ctx, vec![], |ui| {
                facts(
                    ui,
                    &[
                        Fact::new("Páginas atuais", "0"),
                        Fact::new("Sessões", "—"),
                        Fact::new("Com falha", "2").color(Some(p.bad)),
                    ],
                );
            });
            for text in ["Páginas atuais: 0", "Sessões: —", "Com falha: 2"] {
                node(&out, Role::Label, text);
            }
            let texts = texts(&out);
            assert!(texts.contains(&("2".into(), p.bad)), "{theme:?}");
            assert!(texts.contains(&("0".into(), p.text)));
            assert!(texts.contains(&("Sessões".into(), p.text2)));
            let a = rect_of(&node(&out, Role::Label, "Páginas atuais: 0"));
            let b = rect_of(&node(&out, Role::Label, "Sessões: —"));
            assert_eq!(b.top() - a.bottom(), 6., "rows 6 apart");
        }
    }

    #[test]
    fn disclosures_start_closed_and_open_on_click() {
        for (theme, _) in THEMES {
            let ctx = context(theme);
            let mut add = |ui: &mut Ui| {
                disclosure_row(ui, "row", "Métricas", "48 séries", |ui| {
                    ui.label("corpo da linha")
                });
                disclosure_link(ui, "link", "Uso por atividade (6)", |ui| {
                    ui.label("corpo do link")
                });
            };
            let out = run(&ctx, vec![], &mut add);
            let expanded =
                |out: &egui::FullOutput, text| node(out, Role::Button, text).is_expanded();
            assert_eq!(expanded(&out, "Métricas"), Some(false), "{theme:?}");
            assert_eq!(expanded(&out, "Uso por atividade (6)"), Some(false));
            assert!(nodes(&out, Role::Label, "corpo da linha").is_empty());
            assert_eq!(rect_of(&node(&out, Role::Button, "Métricas")).height(), 40.);
            let row = rect_of(&node(&out, Role::Button, "Métricas")).center();
            click(&ctx, row, &mut add);
            let out = run(&ctx, vec![], &mut add);
            assert_eq!(expanded(&out, "Métricas"), Some(true));
            node(&out, Role::Label, "corpo da linha");
            assert!(
                nodes(&out, Role::Label, "corpo do link").is_empty(),
                "each keeps its own state"
            );
            let link = rect_of(&node(&out, Role::Button, "Uso por atividade (6)")).center();
            click(&ctx, link, &mut add);
            let out = run(&ctx, vec![], &mut add);
            node(&out, Role::Label, "corpo do link");
            click(&ctx, row, &mut add);
            let out = run(&ctx, vec![], &mut add);
            assert_eq!(
                expanded(&out, "Métricas"),
                Some(false),
                "a second click closes"
            );
            assert!(nodes(&out, Role::Label, "corpo da linha").is_empty());
        }
    }
}
