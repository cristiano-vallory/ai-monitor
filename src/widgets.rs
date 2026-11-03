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
//! - `confirm_sheet(ui, &Confirm { title, subtitle, facts, preview, warning,
//!   typed, verb, left, ready }, &mut texto, |ui| prévia) -> Option<Answer>`:
//!   a folha de confirmação (Review.dc.html). Desce do topo da área de
//!   conteúdo (o `CentralPanel`), centrada, com largura min(540, área − 32),
//!   raio 12 só embaixo, sobre um véu de 45 % de preto que cobre a área e
//!   engole os cliques fora dela; a lateral e a barra superior seguem
//!   clicáveis. Título (15/600), linha em text2, `details(facts)`, a prévia
//!   (cabeçalho 11 sobre surf2; rola além de 240), `strip` Warn com
//!   `warning`; com `typed: Some(alvo)`, "Digite alvo para confirmar" e um
//!   `text_field` (até 512 caracteres) que pega o foco quando nada mais o
//!   tem. Rodapé: "Prévia válida por m:ss" (de `left`, arredondado para
//!   cima; zero fecha a folha), Cancelar e o botão final:
//!   `Button::primary(verb)` sem digitação, `Button::danger(verb)` com ela,
//!   habilitado só com `ready` e o texto exato. Esc ou Cancelar =
//!   `Answer::Cancel`; o botão final = `Answer::Confirm`. Pede um repaint
//!   por segundo enquanto aberta. Segue o `ui` desabilitado (botões e Esc).
//! - `card_grid(ui, &["Captura", …], |ui, i| …)`: um cartão por título, numa
//!   grade `repeat(auto-fit, minmax(260px, 1fr))` com gap 12. As colunas saem da
//!   largura disponível a cada frame (`grid_columns`); nada do layout anterior
//!   é guardado. Alturas iguais na linha. Cada cartão pinta o título (Seção) e
//!   é um nó AccessKit `Pane` com o título como rótulo.
//! - `grid_columns(largura, itens) -> usize`: a conta da grade acima.
//! - `toolbar(ui, |ui| esquerda, |ui| direita)`: linha de ferramentas no topo do
//!   conteúdo da página; a direita é montada da direita para a esquerda (a
//!   1ª chamada fica na borda), e cada componente mantém a ordem interna (um
//!   `segmented` ali segue da esquerda para a direita).
//!
//! Controles (todos respeitam `ui.add_enabled` / `add_enabled_ui`)
//! - `ui.add(Button::primary("Salvar"))`, `Button::secondary`,
//!   `Button::destructive`, `Button::danger` (preenchido em bad, texto
//!   on_acc em SemiBold: o botão final da confirmação com digitação);
//!   `.small()` = 24 px e texto 12 (padrão: 28 px, 13); `.on_surf()` = fundo
//!   bg, para quando o botão fica sobre surf. Raio 6. Desabilitado:
//!   `ui.add_enabled(false, Button::…)` pinta bg/line/text e o egui esmaece
//!   (≈ o cinza do artboard); o `danger`, bad a 35 %. Rótulo ≥ 4,5:1 sobre o
//!   fundo em todos, nos dois temas. AccessKit: `Button`, com `disabled`
//!   quando desabilitado.
//! - `text_field(ui, &mut texto, largura, dica, senha, limite) -> Response`:
//!   campo de 26 px, raio 5, fundo field, borda line (acc com foco), texto 12
//!   e a `dica` em text2; `senha = true` mascara o texto; `limite` em
//!   caracteres (o que passa de uma colagem é cortado). AccessKit:
//!   `TextInput` (`PasswordInput` com senha).
//! - `checkbox(ui, &mut marcado, rótulo) -> Response`: caixa 14 × 14, raio 3,
//!   rótulo 13 a 8; desmarcada, fundo field e borda text2; marcada, fundo acc
//!   e o visto on_acc. A borda (ou o fundo) tem ≥ 3:1 sobre bg, surf e surf2
//!   nos dois temas (pior caso 4,70:1; a do egui, 1,25:1 sobre surf no
//!   escuro).
//!   `changed()` no clique. AccessKit: `CheckBox` marcado/desmarcado.
//! - `segmented(ui, &mut atual, &[Segment { value, label, count }]) -> bool`
//!   (true = mudou). Fundo surf2, borda line, raio 7, itens de 24 px, texto 12;
//!   o selecionado tem surf, sombra e SemiBold; `count` vai em text2.
//!   AccessKit: um `Button` por item, rótulo "label count", marcado = selecionado.
//!   `segmented_width(ui, &atual, &itens)`: a largura que ele ocupa, antes de
//!   desenhar (para decidir a quebra de uma linha).
//! - `switch(ui, &mut ligado, rótulo) -> Response`: interruptor 26 × 16 com
//!   rótulo; `changed()` no clique. Ligado: trilho acc; desligado: trilho
//!   line com borda e botão text2 (≥ 3:1 sobre o fundo). AccessKit:
//!   `CheckBox` marcado/desmarcado.
//! - `disclosure_row(ui, id, título, meta, |ui| corpo) -> Option<R>`: linha de
//!   40 px com seta, título e `meta` (text2) à direita; `disclosure_link(ui, id,
//!   título, |ui| corpo) -> Option<R>`: seta e título em 12, cor acc. Recolhidos
//!   por padrão; o estado fica na memória do egui (`id` + id da `Ui`). Devolvem
//!   o retorno do corpo quando aberto. AccessKit: `Button` com `expanded`.
//!
//! Estado e medidas
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
//! - `details(ui, &[Fact::new(rótulo, valor)…])`: a variante do inspetor do
//!   Docker: rótulos (12, text2) numa coluna da largura do maior e valores
//!   (12, Regular, `cor` ou text; `.mono()` = `Type::Mono`) à esquerda, que
//!   quebram linha em vez de cortar; linhas a 8, colunas a 14. AccessKit:
//!   um `Label` "rótulo: valor" por linha.
//! - `tone(cor, alfa) -> Color32`: o tom translúcido de chips e faixas.
//! - `chevron(painter, rect, aberto, cor)`: a seta fina dos recolhíveis (para
//!   baixo se aberto), também a dos filtros (`ComboBox::icon`).
//! - `strokes(painter, rect, &[&[(x, y), …]], largura, cor)` e `dot(painter,
//!   rect, (x, y), largura, cor)`: ícones a partir dos paths de 24 unidades dos
//!   artboards (um traço que volta ao ponto inicial é fechado).
//!
//! Páginas
//! - `one_line(ui, texto, fonte, cor, largura) -> Arc<Galley>`: uma linha, com
//!   reticências além de `largura` (`galley.elided` diz se cortou).
//! - `pane(ui, rect, rótulo)`: nó AccessKit `Pane` (o aria-label do artboard).
//! - `empty(ui, título, subtítulo)`: mensagem centrada no fluxo (24 de
//!   respiro, título Seção, subtítulo 12 text2 que quebra).
//! - `search_field(ui, &mut texto, largura, dica)`: campo de busca de 12 px
//!   com a lupa à esquerda e a `dica` como placeholder em text2.
//! - `split(ui, &[legenda], |ui| conteúdo) -> R`: dá ao conteúdo a altura que
//!   sobra (mín. 120) e pinta as linhas da legenda (`caption`) embaixo.
//! - `table(ui, id, &[Column { title, width, hint }], TableSize { head, row,
//!   pad }, linhas, topo, |ui, linha, coluna| …)`: cabeçalho (11, text2; o
//!   `hint` vai ao tooltip) e linhas de `row` com divisória, na largura e na
//!   altura de `ui.max_rect()` a cada frame; `width: None` = a coluna que
//!   estica (mín. 120). Só as linhas visíveis são montadas; `topo` volta a
//!   rolagem ao início. A barra de rolagem é a de `scroll_bar`.
//! - `scroll_bar(ui)`: nas listas e tabelas, a barra de rolagem à vista
//!   (4 px, text2) sempre que há mais linhas que a área; sem transbordo,
//!   nenhuma. Chamar antes de montar a `ScrollArea`.
use crate::theme;
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, FontId, Galley, Layout, Pos2, Rect, Response,
    Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use std::{hash::Hash, sync::Arc, time::Duration};

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
    Danger,
}
pub(crate) struct Button<'a> {
    text: &'a str,
    kind: Kind,
    small: bool,
    on_surf: bool,
}
impl<'a> Button<'a> {
    pub(crate) fn primary(text: &'a str) -> Self {
        Self::new(text, Kind::Primary)
    }
    pub(crate) fn secondary(text: &'a str) -> Self {
        Self::new(text, Kind::Secondary)
    }
    #[allow(dead_code, reason = "Fase 4: confirmações")]
    pub(crate) fn destructive(text: &'a str) -> Self {
        Self::new(text, Kind::Destructive)
    }
    /// Preenchido em bad: o botão final da confirmação com digitação.
    pub(crate) fn danger(text: &'a str) -> Self {
        Self::new(text, Kind::Danger)
    }
    fn new(text: &'a str, kind: Kind) -> Self {
        Self {
            text,
            kind,
            small: false,
            on_surf: false,
        }
    }
    pub(crate) fn small(mut self) -> Self {
        self.small = true;
        self
    }
    /// Sobre um fundo surf (o inspetor do Docker): secundário e destrutivo
    /// com fundo bg, que contrasta com ele.
    pub(crate) fn on_surf(mut self) -> Self {
        self.on_surf = true;
        self
    }
}
impl egui::Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let p = theme::palette(ui.visuals());
        let enabled = ui.is_enabled();
        let base = if self.on_surf { p.bg } else { p.surf };
        let filled = matches!(self.kind, Kind::Primary | Kind::Danger);
        let (fill, stroke, color) = match (enabled, self.kind) {
            // O egui esmaece o desabilitado (`disabled_alpha`): bad a
            // 0,35 / alpha sai bad a 35 %, como no artboard.
            (false, Kind::Danger) => (
                tone(p.bad, 0.35 / ui.visuals().disabled_alpha()),
                None,
                p.on_acc,
            ),
            (false, _) => (p.bg, Some(p.line), p.text),
            (true, Kind::Primary) => (p.acc, None, p.on_acc),
            // on_acc: o branco do artboard no claro; no escuro, o texto
            // escuro do destaque, que contrasta com o bad claro.
            (true, Kind::Danger) => (p.bad, None, p.on_acc),
            (true, Kind::Secondary) => (base, Some(p.line), p.text),
            (true, Kind::Destructive) => (base, Some(p.line), p.bad),
        };
        let family = if filled {
            theme::semibold()
        } else {
            FontFamily::Proportional
        };
        let (size, height, pad) = match (self.small, filled) {
            (true, _) => (12., 24., 10.),
            (false, true) => (13., 28., 14.),
            (false, false) => (13., 28., 12.),
        };
        let galley =
            ui.painter()
                .layout_no_wrap(self.text.to_owned(), FontId::new(size, family), color);
        let (rect, response) =
            ui.allocate_exact_size(vec2(galley.size().x + 2. * pad, height), Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, self.text));
        let fill = if enabled && !filled && response.hovered() {
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
/// O texto de um item (o selecionado em SemiBold) e o rótulo falado.
fn segment_text<T>(
    p: &theme::Palette,
    item: &Segment<'_, T>,
    selected: bool,
) -> (egui::text::LayoutJob, String) {
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
    (job, label)
}
/// A largura dos itens (texto + 24 cada), sem a moldura.
fn items_width<T: PartialEq + Copy>(ui: &Ui, current: &T, items: &[Segment<'_, T>]) -> f32 {
    let p = theme::palette(ui.visuals());
    items
        .iter()
        .map(|item| {
            let (job, _) = segment_text(p, item, *current == item.value);
            ui.painter().layout_job(job).size().x + 24.
        })
        .sum()
}
/// A largura de `segmented` com estes itens: os itens (texto + 24), a margem
/// de 2 e a borda de 1 dos dois lados.
pub(crate) fn segmented_width<T: PartialEq + Copy>(
    ui: &Ui,
    current: &T,
    items: &[Segment<'_, T>],
) -> f32 {
    items_width(ui, current, items) + 2. * 2. + 2. * 1.
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
            // Uma linha de 24 desde o início: o `horizontal` começa com a
            // altura de interação (18) e, numa linha centrada, desceria e
            // esticaria a moldura. Na largura exata dos itens: num leiaute da
            // direita para a esquerda (o lado direito de `toolbar`), a
            // moldura encosta à direita com os itens na ordem.
            ui.allocate_ui_with_layout(
                vec2(items_width(ui, current, items), 24.),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.;
                    for item in items {
                        let selected = *current == item.value;
                        let color = if selected { p.text } else { p.text2 };
                        let (job, label) = segment_text(p, item, selected);
                        let galley = ui.painter().layout_job(job);
                        let (rect, response) = ui
                            .allocate_exact_size(vec2(galley.size().x + 24., 24.), Sense::click());
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
                },
            );
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
    // Desligado: trilho line com borda text2 e o botão text2, que se veem
    // (≥ 3:1) sobre bg, surf e surf2 nos dois temas; o trilho line sozinho
    // some no fundo. Ligado: o botão claro do artboard, surf no tema claro
    // (branco) e text no escuro.
    let knob = if !*on {
        painter.rect(
            track,
            8.,
            p.line,
            Stroke::new(1.0_f32, p.text2),
            StrokeKind::Inside,
        );
        p.text2
    } else {
        painter.rect_filled(track, 8., p.acc);
        if ui.visuals().dark_mode {
            p.text
        } else {
            p.surf
        }
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

#[allow(
    dead_code,
    reason = "Fase 4: Ajustes, Revisão, Explorar e Administração"
)]
pub(crate) fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    let p = theme::palette(ui.visuals());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), Type::Body.font(), p.text);
    let size = vec2(14. + 8. + galley.size().x, galley.size().y.max(14.));
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *checked, label)
    });
    let tick = Rect::from_min_size(pos2(rect.left(), rect.center().y - 7.), Vec2::splat(14.));
    let painter = ui.painter();
    // Desmarcada: borda text2, que se vê (≥ 3:1) sobre bg, surf e surf2 nos
    // dois temas; a borda line do egui fica entre 1,1 e 1,4:1. Marcada:
    // fundo acc e o visto em on_acc.
    if *checked {
        painter.rect_filled(tick, 3., p.acc);
        strokes(
            painter,
            tick,
            &[&[(5., 12.5), (10., 17.5), (19., 7.5)]],
            3.,
            p.on_acc,
        );
    } else {
        painter.rect(
            tick,
            3.,
            p.field,
            Stroke::new(1.0_f32, p.text2),
            StrokeKind::Inside,
        );
    }
    painter.galley(
        pos2(tick.right() + 8., rect.center().y - galley.size().y / 2.),
        galley,
        p.text,
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
/// Texto numa linha, com reticências além de `width`.
pub(crate) fn one_line(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> Arc<Galley> {
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
            let value = one_line(
                ui,
                &fact.value,
                font,
                value_color,
                width - 12. - label_width.min(width * 0.4),
            );
            let label = one_line(
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

/// A lista do inspetor (o `dl` do Docker.dc.html): rótulos (12, text2) numa
/// coluna da largura do maior; valores (12, Regular, `cor` ou text;
/// `.mono()` = `Type::Mono`) alinhados à esquerda, que quebram linha (em
/// qualquer ponto, se preciso) em vez de cortar. Linhas a 8, colunas a 14.
/// AccessKit: um `Label` "rótulo: valor" por linha.
pub(crate) fn details(ui: &mut Ui, rows: &[Fact<'_>]) {
    let p = theme::palette(ui.visuals());
    let labels: Vec<Arc<Galley>> = rows
        .iter()
        .map(|fact| {
            ui.painter()
                .layout_no_wrap(fact.label.to_owned(), Type::Caption.font(), p.text2)
        })
        .collect();
    let column = labels.iter().map(|g| g.size().x).fold(0., f32::max);
    let width = ui.available_width();
    let value_width = (width - column - 14.).max(0.);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 8.;
        for (fact, label) in rows.iter().zip(labels) {
            let color = fact.color.unwrap_or(p.text);
            let font = if fact.mono {
                Type::Mono.font()
            } else {
                Type::Caption.font()
            };
            let value = ui
                .painter()
                .layout(fact.value.clone(), font, color, value_width);
            let (rect, response) = ui.allocate_exact_size(
                vec2(width, value.size().y.max(label.size().y)),
                Sense::hover(),
            );
            let spoken = format!("{}: {}", fact.label, fact.value);
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &spoken));
            ui.painter().galley(rect.min, label, p.text2);
            ui.painter()
                .galley(pos2(rect.left() + column + 14., rect.top()), value, color);
        }
    });
}

/// Nó AccessKit `Pane` com o rótulo do artboard (aria-label).
pub(crate) fn pane(ui: &mut Ui, rect: Rect, label: &str) {
    ui.interact(rect, ui.id().with(label), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Panel, true, label));
}

/// Mensagem centrada no fluxo (o que vem depois segue abaixo dela).
pub(crate) fn empty(ui: &mut Ui, title: &str, subtitle: &str) {
    let p = theme::palette(ui.visuals());
    ui.vertical_centered(|ui| {
        ui.add_space(space::XL);
        ui.label(egui::RichText::new(title).font(Type::Section.font()));
        ui.add(egui::Label::new(egui::RichText::new(subtitle).size(12.).color(p.text2)).wrap());
    });
}

pub(crate) fn search_field(ui: &mut Ui, text: &mut String, width: f32, hint: &str) {
    let p = theme::palette(ui.visuals());
    let response = ui.add(
        egui::TextEdit::singleline(text)
            // Sem cor própria, a dica sairia em `text` (override_text_color)
            // e pareceria texto digitado.
            .hint_text(egui::RichText::new(hint).color(p.text2))
            .font(FontId::proportional(12.))
            .margin(egui::Margin {
                left: 26,
                right: 8,
                top: 6,
                bottom: 6,
            })
            .desired_width(width - 34.),
    );
    let icon = Rect::from_min_size(
        pos2(response.rect.left() + 8., response.rect.center().y - 6.5),
        Vec2::splat(13.),
    );
    let scale = icon.width() / 24.;
    ui.painter().circle_stroke(
        icon.min + vec2(11., 11.) * scale,
        7. * scale,
        Stroke::new(2. * scale, p.text2),
    );
    strokes(
        ui.painter(),
        icon,
        &[&[(20., 20.), (16., 16.)]],
        2.,
        p.text2,
    );
}

pub(crate) fn text_field(
    ui: &mut Ui,
    text: &mut String,
    width: f32,
    hint: &str,
    password: bool,
    limit: usize,
) -> Response {
    let p = theme::palette(ui.visuals());
    // O fundo vem antes do texto, mas a cor da borda depende do foco.
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .frame(false)
            // Sem cor própria, a dica sairia em `text` (override_text_color).
            .hint_text(egui::RichText::new(hint).color(p.text2))
            .font(FontId::proportional(12.))
            .margin(egui::Margin::symmetric(8, 0))
            .min_size(vec2(width, 26.))
            .desired_width(width - 16.)
            .vertical_align(Align::Center)
            .password(password)
            .char_limit(limit),
    );
    let border = if response.has_focus() { p.acc } else { p.line };
    ui.painter().set(
        background,
        egui::epaint::RectShape::new(
            response.rect,
            5.,
            p.field,
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        ),
    );
    response
}

/// Mede a legenda, dá à página o resto da altura e pinta a legenda embaixo.
pub(crate) fn split<R>(ui: &mut Ui, legend: &[String], content: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width();
    let gap = ui.spacing().item_spacing.y;
    let legend_height: f32 = legend
        .iter()
        .map(|line| {
            ui.painter()
                .layout(
                    line.clone(),
                    FontId::proportional(12.),
                    Color32::PLACEHOLDER,
                    width,
                )
                .size()
                .y
                + gap
        })
        .sum();
    let height = (ui.available_height() - legend_height).max(120.);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let mut area = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    let inner = content(&mut area);
    for line in legend {
        caption(ui, line);
    }
    inner
}

/// Listas e tabelas: a barra de rolagem fica à vista (4 px de text2, 10 ao
/// passar o mouse) sempre que há mais linhas que a área, para a lista não
/// parecer completa; sem transbordo, nenhuma. Flutua sobre o respiro direito
/// das linhas, sem mudar a largura delas. Vale para as `ScrollArea` montadas
/// em `ui` depois da chamada.
pub(crate) fn scroll_bar(ui: &mut Ui) {
    let p = theme::palette(ui.visuals());
    ui.spacing_mut().scroll = egui::style::ScrollStyle {
        floating_width: 4.,
        foreground_color: true,
        dormant_handle_opacity: 1.,
        active_handle_opacity: 1.,
        interact_handle_opacity: 1.,
        dormant_background_opacity: 0.,
        active_background_opacity: 0.,
        ..egui::style::ScrollStyle::floating()
    };
    // `foreground_color`: o puxador usa o traço dos widgets.
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.fg_stroke.color = p.text2;
    widgets.hovered.fg_stroke.color = p.text;
    widgets.active.fg_stroke.color = p.text;
}

/// Largura mínima da coluna que estica numa `table`.
const FLEX_MIN: f32 = 120.;
pub(crate) struct Column {
    pub title: &'static str,
    /// `None` = a coluna que estica (o resto, com `FLEX_MIN` no mínimo).
    pub width: Option<f32>,
    /// Dica do cabeçalho (tooltip).
    pub hint: Option<&'static str>,
}
/// Alturas do cabeçalho e das linhas, e o respiro de cada lado das células.
#[derive(Clone, Copy)]
pub(crate) struct TableSize {
    pub head: f32,
    pub row: f32,
    pub pad: f32,
}
/// Cabeçalho (11 px, text2) e linhas de `size.row` com divisória; só as
/// linhas visíveis são montadas. As larguras saem de `ui.max_rect()` a cada
/// frame. `top` volta a rolagem ao início.
pub(crate) fn table(
    ui: &mut Ui,
    id: &str,
    columns: &[Column],
    size: TableSize,
    rows: usize,
    top: bool,
    mut cell: impl FnMut(&mut Ui, usize, usize),
) {
    let p = theme::palette(ui.visuals());
    let rect = ui.max_rect();
    let fixed: f32 = columns.iter().filter_map(|c| c.width).sum();
    let flex = (rect.width() - fixed).max(FLEX_MIN);
    let widths: Vec<f32> = columns.iter().map(|c| c.width.unwrap_or(flex)).collect();
    let mut x = rect.left();
    for (column, width) in columns.iter().zip(&widths) {
        let at = Rect::from_min_size(pos2(x, rect.top()), vec2(*width, size.head))
            .shrink2(vec2(size.pad, 0.));
        let head = ui
            .new_child(
                UiBuilder::new()
                    .max_rect(at)
                    .layout(Layout::left_to_right(Align::Center)),
            )
            .add(
                egui::Label::new(egui::RichText::new(column.title).size(11.).color(p.text2))
                    .truncate(),
            );
        if let Some(hint) = column.hint {
            head.on_hover_text(hint);
        }
        x += width;
    }
    ui.painter().hline(
        rect.x_range(),
        rect.top() + size.head - 0.5,
        Stroke::new(1.0_f32, p.line),
    );
    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + size.head), rect.max);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(body)
            .layout(Layout::top_down(Align::Min)),
    );
    scroll_bar(&mut child);
    let mut area = egui::ScrollArea::vertical().id_salt(id).auto_shrink(false);
    if top {
        area = area.vertical_scroll_offset(0.);
    }
    area.show_viewport(&mut child, |ui, viewport| {
        ui.set_height(size.row * rows as f32);
        let content = ui.max_rect();
        let first = (viewport.min.y / size.row).floor().max(0.) as usize;
        let last = ((viewport.max.y / size.row).ceil().max(0.) as usize).min(rows);
        for row in first..last {
            let y = content.top() + row as f32 * size.row;
            let mut x = content.left();
            for (column, width) in widths.iter().enumerate() {
                let at = Rect::from_min_size(pos2(x, y), vec2(*width, size.row))
                    .shrink2(vec2(size.pad, 0.));
                let mut cell_ui = ui.new_child(
                    UiBuilder::new()
                        .id_salt((row, column))
                        .max_rect(at)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                cell(&mut cell_ui, row, column);
                x += width;
            }
            ui.painter().hline(
                content.x_range(),
                y + size.row - 0.5,
                Stroke::new(1.0_f32, p.line),
            );
        }
    });
}

/// Estado aberto de um recolhível, na memória do egui.
fn disclosure_state(ui: &Ui, id: egui::Id) -> bool {
    ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false)
}
/// A seta dos recolhíveis (e dos filtros): para baixo se `open`, senão para
/// a direita; traço 2,5 no quadro de 24.
pub(crate) fn chevron(painter: &egui::Painter, rect: Rect, open: bool, color: Color32) {
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

/// A decisão tomada na folha de confirmação neste frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Answer {
    Confirm,
    Cancel,
}
/// O conteúdo da folha de confirmação (Review.dc.html).
pub(crate) struct Confirm<'a> {
    /// A ação como pergunta ("Aprovar proposta?").
    pub title: &'a str,
    /// A linha do projeto ou do alvo, abaixo do título.
    pub subtitle: &'a str,
    /// A grade curta (Operação, Alvo…), em `details`.
    pub facts: &'a [Fact<'a>],
    /// O cabeçalho da prévia ("Prévia da alteração").
    pub preview: &'a str,
    /// O texto da faixa de aviso.
    pub warning: &'a str,
    /// `Some(alvo)`: nível Digitação, o botão final só habilita com `alvo`
    /// exato no campo. `None`: Clique.
    pub typed: Option<&'a str>,
    /// O rótulo do botão final ("Aprovar", "Excluir", "Confirmar").
    pub verb: &'a str,
    /// Quanto falta para a prévia vencer; zero fecha a folha.
    pub left: Duration,
    /// Falso desabilita o botão final (operação em andamento).
    pub ready: bool,
}
/// O véu sob a folha: preto a 45 %.
const VEIL: Color32 = Color32::from_black_alpha(115);
/// Altura máxima da prévia; o resto rola.
const PREVIEW_HEIGHT: f32 = 240.;

/// A área de conteúdo: o `CentralPanel` em volta de `ui` (abaixo da barra
/// superior, à direita da lateral).
fn content_area(ui: &Ui) -> Rect {
    ui.stack()
        .iter()
        .find(|frame| frame.kind() == Some(egui::UiKind::CentralPanel))
        .map_or_else(|| ui.ctx().content_rect(), |frame| frame.max_rect)
}

pub(crate) fn confirm_sheet(
    ui: &mut Ui,
    confirm: &Confirm<'_>,
    input: &mut String,
    preview: impl FnOnce(&mut Ui),
) -> Option<Answer> {
    if confirm.left.is_zero() {
        return None;
    }
    let enabled = ui.is_enabled();
    if enabled && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        return Some(Answer::Cancel);
    }
    // A contagem muda a cada segundo.
    ui.ctx().request_repaint_after(Duration::from_secs(1));
    let p = theme::palette(ui.visuals());
    let area = content_area(ui);
    let width = (area.width() - 32.).min(540.);
    let column = Rect::from_min_size(
        pos2(area.center().x - width / 2., area.top()),
        vec2(width, area.height()),
    );
    let mut answer = None;
    // Uma camada acima da página, do tamanho da área de conteúdo: o clique
    // fora da folha cai no véu, não no que está embaixo. A lateral e a barra
    // superior ficam fora dela.
    egui::Area::new(egui::Id::new("confirmation-sheet"))
        .order(egui::Order::Middle)
        .fixed_pos(area.min)
        .constrain_to(area)
        .default_size(area.size())
        .fade_in(false)
        .enabled(enabled)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            ui.painter().rect_filled(area, 0., VEIL);
            let mut column = ui.new_child(
                UiBuilder::new()
                    .max_rect(column)
                    .layout(Layout::top_down(Align::Min)),
            );
            sheet(&column)
                .corner_radius(CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 12,
                    se: 12,
                })
                .inner_margin(egui::Margin {
                    left: 22,
                    right: 22,
                    top: 20,
                    bottom: 18,
                })
                .show(&mut column, |ui| {
                    // A largura toda menos as margens (22 + 22) e a borda (1 + 1).
                    ui.set_width(width - 46.);
                    ui.spacing_mut().item_spacing.y = 14.;
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = space::XS;
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(confirm.title).font(Type::Bar.font()),
                            )
                            .wrap(),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(confirm.subtitle)
                                    .font(Type::Body.font())
                                    .color(p.text2),
                            )
                            .wrap(),
                        );
                    });
                    // Numa janela baixa o meio rola, e o rodapé (28 + 14 de
                    // espaço) fica à vista acima da margem (18) e de 16 de
                    // respiro.
                    let body = area.bottom() - 16. - 18. - 28. - 14. - ui.cursor().top();
                    egui::ScrollArea::vertical()
                        .id_salt("body")
                        .max_height(body.max(80.))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 14.;
                            details(ui, confirm.facts);
                            preview_box(ui, confirm.preview, preview);
                            strip(ui, Tone::Warn, "Atenção.", confirm.warning, None);
                            if let Some(target) = confirm.typed {
                                typing(ui, target, input);
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.set_min_height(28.);
                        let left =
                            confirm.left.as_secs() + u64::from(confirm.left.subsec_nanos() > 0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Prévia válida por {}:{:02}",
                                left / 60,
                                left % 60
                            ))
                            .font(Type::Caption.font())
                            .color(p.text2),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = space::S;
                            let ok =
                                confirm.ready && confirm.typed.is_none_or(|t| input.as_str() == t);
                            let button = if confirm.typed.is_some() {
                                Button::danger(confirm.verb)
                            } else {
                                Button::primary(confirm.verb)
                            };
                            if ui.add_enabled(ok, button).clicked() {
                                answer = Some(Answer::Confirm);
                            }
                            if ui.add(Button::secondary("Cancelar")).clicked() {
                                answer = Some(Answer::Cancel);
                            }
                        });
                    });
                });
        });
    answer
}
/// A caixa da prévia: borda line, cabeçalho (11, text2) sobre surf2 e o
/// conteúdo com rolagem até `PREVIEW_HEIGHT`.
fn preview_box(ui: &mut Ui, title: &str, content: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.visuals());
    egui::Frame::new()
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(8.)
        .inner_margin(1)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            egui::Frame::new()
                .fill(p.surf2)
                .corner_radius(CornerRadius {
                    nw: 7,
                    ne: 7,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(egui::RichText::new(title).size(11.).color(p.text2));
                });
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("preview")
                        .max_height(PREVIEW_HEIGHT)
                        .auto_shrink([false, true])
                        .show(ui, content);
                });
        });
}
/// "Digite <alvo> para confirmar" (o alvo em mono) e o campo, com o foco
/// quando nada mais o tem.
fn typing(ui: &mut Ui, target: &str, input: &mut String) {
    let p = theme::palette(ui.visuals());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = ui.available_width();
        for (text, font) in [
            ("Digite ", Type::Caption.font()),
            (target, FontId::monospace(12.)),
            (" para confirmar", Type::Caption.font()),
        ] {
            job.append(text, 0., egui::TextFormat::simple(font, p.text));
        }
        let label = ui.add(egui::Label::new(job));
        // O limite do campo antigo: um texto colado não passa de 512.
        let field =
            text_field(ui, input, ui.available_width(), "", false, 512).labelled_by(label.id);
        if ui.memory(|m| m.focused().is_none()) {
            field.request_focus();
        }
    });
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
    /// Os eventos de um clique completo em `pos`, um frame por item: o
    /// ponteiro chega, aperta e solta. Como o mouse: chegar e apertar no
    /// mesmo frame, numa rolagem, vira arrasto e o clique se perde.
    pub(crate) fn click_frames(pos: Pos2) -> [Vec<egui::Event>; 3] {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        [
            vec![egui::Event::PointerMoved(pos)],
            vec![press(true)],
            vec![press(false)],
        ]
    }
    /// Clique completo em `pos` (`click_frames`).
    pub(crate) fn click(ctx: &egui::Context, pos: Pos2, add: impl FnMut(&mut Ui)) {
        click_sized(ctx, [800., 600.], pos, add);
    }
    pub(crate) fn click_sized(
        ctx: &egui::Context,
        size: [f32; 2],
        pos: Pos2,
        mut add: impl FnMut(&mut Ui),
    ) {
        for events in click_frames(pos) {
            run_sized(ctx, size, events, &mut add);
        }
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

    /// Os puxadores de rolagem pintados em `area`: retângulos `color` de até
    /// 10 px encostados na borda direita.
    pub(crate) fn scroll_handles(out: &egui::FullOutput, area: Rect, color: Color32) -> Vec<Rect> {
        rects(out)
            .into_iter()
            .filter(|r| {
                r.fill == color
                    && (r.rect.right() - area.right()).abs() < 0.5
                    && r.rect.width() <= 10.
                    && area.expand(0.5).contains_rect(r.rect)
            })
            .map(|r| r.rect)
            .collect()
    }

    #[test]
    fn a_table_with_more_rows_than_its_area_shows_its_scroll_bar() {
        let columns = [Column {
            title: "Nome",
            width: None,
            hint: None,
        }];
        let size = TableSize {
            head: 28.,
            row: 36.,
            pad: 6.,
        };
        for (theme, p) in THEMES {
            // (linhas, puxador?): 10 × 36 = 360 numa área de 372 − 28 = 344.
            for (rows, shown) in [(0, false), (9, false), (10, true), (100, true)] {
                let ctx = context(theme);
                let mut add = |ui: &mut Ui| {
                    ui.allocate_ui(vec2(400., 372.), |ui| {
                        let rect = ui.max_rect();
                        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                            table(ui, "t", &columns, size, rows, false, |ui, row, _| {
                                ui.label(format!("linha {row}"));
                            });
                        });
                    });
                };
                // A barra aparece com uma animação curta (1/60 s por frame).
                let mut out = run(&ctx, vec![], &mut add);
                for _ in 0..10 {
                    out = run(&ctx, vec![], &mut add);
                }
                let body = Rect::from_min_max(pos2(8., 8. + 28.), pos2(408., 8. + 372.));
                let handles = scroll_handles(&out, body, p.text2);
                let case = format!("{theme:?} {rows}");
                assert_eq!(handles.len(), usize::from(shown), "{case}: {handles:?}");
                if let Some(handle) = handles.first() {
                    assert_eq!(handle.width(), 4., "{case}");
                    // Proporcional ao que se vê: 344 de 36 × linhas.
                    let want = body.height() * body.height() / (36. * rows as f32);
                    assert!(
                        (handle.height() - want.max(12.)).abs() < 1.,
                        "{case}: {handle:?}, want {want}"
                    );
                }
            }
        }
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
                (Kind::Danger, p.bad, p.on_acc),
            ] {
                // Um tipo novo tem de entrar nesta tabela.
                match kind {
                    Kind::Primary | Kind::Secondary | Kind::Destructive | Kind::Danger => {}
                }
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
                // Desabilitado: bg/line/text, esmaecido pelo egui (disabled_alpha);
                // o Danger, bad a 35 % (Tokens.dc.html).
                let alpha = ctx.style().visuals.disabled_alpha();
                let off_rect = rect_of(&off);
                let off_fill: Vec<_> = painted
                    .iter()
                    .filter(|r| r.rect == off_rect)
                    .map(|r| r.fill)
                    .collect();
                if kind == Kind::Danger {
                    let [fill] = off_fill[..] else {
                        panic!("{theme:?}: one disabled fill, got {off_fill:?}")
                    };
                    let want = p.bad.gamma_multiply(0.35);
                    assert!(
                        fill.to_array()
                            .iter()
                            .zip(want.to_array())
                            .all(|(a, b)| a.abs_diff(b) <= 1),
                        "{theme:?}: disabled danger {fill:?}, want bad at 35% {want:?}"
                    );
                } else {
                    assert_eq!(
                        off_fill,
                        vec![p.bg.gamma_multiply(alpha)],
                        "{theme:?} {kind:?}: disabled fill"
                    );
                }
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

    #[test]
    fn every_button_label_reads_at_4_5_to_1_on_its_painted_fill() {
        use crate::theme::tests::contrast;
        for (theme, p) in THEMES {
            for on_surf in [false, true] {
                for kind in [
                    Kind::Primary,
                    Kind::Secondary,
                    Kind::Destructive,
                    Kind::Danger,
                ] {
                    let ctx = context(theme);
                    let out = run(&ctx, vec![], |ui| {
                        let button = Button::new("Rótulo", kind);
                        ui.add(if on_surf { button.on_surf() } else { button });
                    });
                    let at = rect_of(&node(&out, Role::Button, "Rótulo"));
                    let fills: Vec<Color32> = rects(&out)
                        .iter()
                        .filter(|r| r.rect == at)
                        .map(|r| r.fill)
                        .collect();
                    let [fill] = fills[..] else {
                        panic!("{theme:?} {kind:?}: one fill, got {fills:?}")
                    };
                    let (_, text) = texts(&out)
                        .into_iter()
                        .find(|(t, _)| t == "Rótulo")
                        .expect("the label is painted");
                    let base = if on_surf { p.bg } else { p.surf };
                    let want = match kind {
                        Kind::Primary => (p.acc, p.on_acc),
                        Kind::Secondary => (base, p.text),
                        Kind::Destructive => (base, p.bad),
                        Kind::Danger => (p.bad, p.on_acc),
                    };
                    assert_eq!((fill, text), want, "{theme:?} {kind:?} on_surf {on_surf}");
                    // Opacos: o contraste só vale entre cores sem alfa.
                    assert_eq!((fill.a(), text.a()), (255, 255), "{theme:?} {kind:?}");
                    let ratio = contrast(text, fill);
                    assert!(
                        ratio >= 4.5,
                        "{theme:?} {kind:?} on_surf {on_surf}: {ratio:.2}:1"
                    );
                }
            }
        }
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
    fn segmented_width_is_the_drawn_width_whatever_is_selected() {
        for (theme, _) in THEMES {
            for selected in [View::A, View::B, View::C] {
                let ctx = context(theme);
                let items = [
                    Segment {
                        value: View::A,
                        label: "CPU",
                        count: None,
                    },
                    Segment {
                        value: View::B,
                        label: "Memória",
                        count: Some("1.234".into()),
                    },
                    Segment {
                        value: View::C,
                        label: "10 min",
                        count: None,
                    },
                ];
                let (mut want, mut drawn) = (0., 0.);
                run(&ctx, vec![], |ui| {
                    let mut current = selected;
                    want = segmented_width(ui, &current, &items);
                    drawn = ui
                        .scope(|ui| segmented(ui, &mut current, &items))
                        .response
                        .rect
                        .width();
                });
                assert!(drawn > 100., "{theme:?} {selected:?}: precondition {drawn}");
                assert_eq!(want, drawn, "{theme:?} {selected:?}");
            }
        }
    }

    #[test]
    fn segmented_is_a_30_px_frame_centered_in_any_row() {
        for (theme, _) in THEMES {
            for row in [24., 28., 30., 44.] {
                let ctx = context(theme);
                let (mut a, mut b) = (View::A, 1);
                let mut frames = Vec::new();
                let out = run(&ctx, vec![], |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(row);
                        ui.label("Título");
                        for _ in 0..2 {
                            frames.push(
                                ui.scope(|ui| {
                                    segmented(
                                        ui,
                                        &mut a,
                                        &[Segment {
                                            value: View::A,
                                            label: "CPU",
                                            count: None,
                                        }],
                                    )
                                })
                                .response
                                .rect,
                            );
                        }
                        segmented(
                            ui,
                            &mut b,
                            &[Segment {
                                value: 1,
                                label: "1 min",
                                count: None,
                            }],
                        );
                    });
                });
                let cpu = nodes(&out, Role::Button, "CPU");
                let one = rect_of(&node(&out, Role::Button, "1 min"));
                assert_eq!(cpu.len(), 2);
                let tops: Vec<f32> = cpu.iter().map(|n| rect_of(n).top()).collect();
                assert!(
                    tops.iter().all(|&t| t == one.top()),
                    "{theme:?} {row}: {tops:?} / {one:?}"
                );
                assert!(
                    frames
                        .iter()
                        .all(|f| f.height() == 30. && f.top() + 3. == one.top()),
                    "{theme:?} {row}: {frames:?} / {one:?}"
                );
            }
        }
    }

    #[test]
    fn a_segmented_on_the_right_of_a_toolbar_keeps_its_order_against_the_edge() {
        let segments = |count: &str| {
            [View::A, View::B, View::C].map(|value| Segment {
                value,
                label: match value {
                    View::A => "CPU",
                    View::B => "Memória",
                    View::C => "10 min",
                },
                count: Some(count.to_owned()),
            })
        };
        for (theme, _) in THEMES {
            let ctx = context(theme);
            let (mut a, mut b) = (View::A, View::C);
            let out = run(&ctx, vec![], |ui| {
                toolbar(
                    ui,
                    |ui| {
                        ui.label("Esquerda");
                    },
                    |ui| {
                        // A primeira chamada fica na borda direita.
                        segmented(ui, &mut a, &segments("1"));
                        segmented(ui, &mut b, &segments("2"));
                    },
                );
            });
            let at = |text: &str| rect_of(&node(&out, Role::Button, text));
            for count in ["1", "2"] {
                let items = ["CPU", "Memória", "10 min"].map(|l| at(&format!("{l} {count}")));
                assert!(
                    items.windows(2).all(|w| w[0].right() == w[1].left()),
                    "{theme:?} {count}: left to right, side by side: {items:?}"
                );
            }
            let (first, second) = (at("10 min 1"), at("CPU 2"));
            // Borda direita do painel (800 − 8), menos a borda e a margem.
            assert_eq!(first.right(), 800. - 8. - 1. - 2., "{theme:?}");
            assert!(
                second.right() < at("CPU 1").left(),
                "{theme:?}: the second call is left of the first"
            );
            assert!(at("CPU 2").left() > rect_of(&node(&out, Role::Label, "Esquerda")).right());
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
    fn the_switch_is_visible_against_every_background_on_and_off() {
        use crate::theme::tests::contrast;
        for (theme, p) in THEMES {
            for on in [false, true] {
                let ctx = context(theme);
                let mut state = on;
                let out = run(&ctx, vec![], |ui| {
                    switch(ui, &mut state, "Só abertas");
                });
                let tracks: Vec<_> = rects(&out)
                    .into_iter()
                    .filter(|r| r.rect.size() == vec2(26., 16.))
                    .collect();
                assert_eq!(tracks.len(), 1, "{theme:?} {on}");
                let track = &tracks[0];
                let knobs: Vec<Color32> = shapes(&out)
                    .into_iter()
                    .filter_map(|s| match s {
                        egui::Shape::Circle(c) if c.radius == 6. => Some(c.fill),
                        _ => None,
                    })
                    .collect();
                assert_eq!(knobs.len(), 1, "{theme:?} {on}");
                // A cor que delimita o componente: a borda, se houver; senão
                // o trilho.
                let edge = if track.stroke.width > 0. {
                    track.stroke.color
                } else {
                    track.fill
                };
                let want = if on {
                    (
                        p.acc,
                        Stroke::NONE,
                        if theme == egui::Theme::Dark {
                            p.text
                        } else {
                            p.surf
                        },
                    )
                } else {
                    (p.line, Stroke::new(1.0_f32, p.text2), p.text2)
                };
                assert_eq!((track.fill, track.stroke, knobs[0]), want, "{theme:?} {on}");
                for (name, back) in [("bg", p.bg), ("surf", p.surf), ("surf2", p.surf2)] {
                    let ratio = contrast(edge, back);
                    assert!(
                        ratio >= 3.,
                        "{theme:?} {on}: the switch on {name} is {ratio:.2}:1"
                    );
                }
                // Desligado, o botão é o que marca o estado. Ligado, é o
                // trilho acc (o botão claro sobre o acc é o do artboard).
                if !on {
                    let ratio = contrast(knobs[0], track.fill);
                    assert!(ratio >= 3., "{theme:?}: knob on track {ratio:.2}:1");
                }
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
    fn the_search_placeholder_is_text2_and_typed_text_is_text() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            for (typed, want) in [
                ("", ("Nome, imagem ou ID", p.text2)),
                ("redis", ("redis", p.text)),
            ] {
                let mut text = typed.to_owned();
                let out = run(&ctx, vec![], |ui| {
                    search_field(ui, &mut text, 200., "Nome, imagem ou ID");
                });
                // O egui pinta também o texto vazio do campo.
                let painted: Vec<_> = texts(&out)
                    .into_iter()
                    .filter(|(t, _)| !t.is_empty())
                    .collect();
                assert_eq!(
                    painted,
                    vec![(want.0.to_owned(), want.1)],
                    "{theme:?} {typed:?}"
                );
                let field = out
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .find(|(_, n)| n.role() == Role::TextInput)
                    .map(|(_, n)| n.placeholder().map(str::to_owned))
                    .expect("the field");
                assert_eq!(field.as_deref(), Some("Nome, imagem ou ID"));
            }
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

    #[test]
    fn the_text_field_is_26_px_on_field_with_a_line_border_and_masks_passwords() {
        for (theme, p) in THEMES {
            for (typed, password, painted) in [
                ("", false, ("Opcional", p.text2)),
                ("ai-memory", false, ("ai-memory", p.text)),
                ("segredo", true, ("•••••••", p.text)),
            ] {
                let ctx = context(theme);
                let mut text = typed.to_owned();
                let out = run(&ctx, vec![], |ui| {
                    text_field(ui, &mut text, 220., "Opcional", password, 64);
                });
                let case = format!("{theme:?} {typed:?} {password}");
                let role = if password {
                    Role::PasswordInput
                } else {
                    Role::TextInput
                };
                let fields: Vec<Rect> = out
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .filter(|(_, n)| n.role() == role)
                    .map(|(_, n)| rect_of(n))
                    .collect();
                let [field] = fields[..] else {
                    panic!("{case}: one {role:?}, got {fields:?}")
                };
                assert_eq!(field.size(), vec2(220., 26.), "{case}");
                let frame: Vec<_> = rects(&out)
                    .into_iter()
                    .filter(|r| r.rect == field)
                    .map(|r| (r.fill, r.stroke, r.corner_radius))
                    .collect();
                assert_eq!(
                    frame,
                    vec![(p.field, Stroke::new(1.0_f32, p.line), CornerRadius::same(5))],
                    "{case}"
                );
                let shown: Vec<_> = texts(&out)
                    .into_iter()
                    .filter(|(t, _)| !t.is_empty())
                    .collect();
                assert_eq!(shown, vec![(painted.0.to_owned(), painted.1)], "{case}");
                assert!(
                    !shown.iter().any(|(t, _)| t.contains("segredo")),
                    "{case}: the password is never painted"
                );
            }
        }
    }

    #[test]
    fn the_focused_text_field_draws_its_border_in_acc() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let mut text = String::new();
            let mut add = |ui: &mut Ui| {
                text_field(ui, &mut text, 200., "", false, 64);
            };
            let out = run(&ctx, vec![], &mut add);
            let field = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, n)| n.role() == Role::TextInput)
                .map(|(_, n)| rect_of(n))
                .expect("the field");
            let border = |out: &egui::FullOutput| {
                rects(out)
                    .into_iter()
                    .find(|r| r.rect == field)
                    .map(|r| r.stroke.color)
            };
            assert_eq!(border(&out), Some(p.line), "{theme:?}: unfocused");
            click(&ctx, field.center(), &mut add);
            let out = run(&ctx, vec![], &mut add);
            assert_eq!(border(&out), Some(p.acc), "{theme:?}: focused");
        }
    }

    #[test]
    fn checkbox_toggles_and_reports_checked_state() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let mut on = false;
            let mut changed = 0;
            let mut add = |ui: &mut Ui| {
                if checkbox(ui, &mut on, "Guardar no Keychain").changed() {
                    changed += 1;
                }
            };
            let out = run(&ctx, vec![], &mut add);
            let state =
                |out: &egui::FullOutput| node(out, Role::CheckBox, "Guardar no Keychain").toggled();
            assert_eq!(state(&out), Some(Toggled::False), "{theme:?}");
            let at = rect_of(&node(&out, Role::CheckBox, "Guardar no Keychain")).center();
            click(&ctx, at, &mut add);
            let out = run(&ctx, vec![], &mut add);
            assert_eq!(state(&out), Some(Toggled::True));
            assert!(
                rects(&out)
                    .iter()
                    .any(|r| r.fill == p.acc && r.rect.size() == vec2(14., 14.)),
                "{theme:?}: the checked box is acc"
            );
            click(&ctx, at, &mut add);
            drop(add);
            assert_eq!((on, changed), (false, 2));
        }
    }

    #[test]
    fn the_checkbox_edge_has_3_to_1_against_every_background_unlike_the_egui_one() {
        use crate::theme::tests::contrast;
        let mut lowest = f64::INFINITY;
        for (theme, p) in THEMES {
            for on in [false, true] {
                let ctx = context(theme);
                let mut state = on;
                let out = run(&ctx, vec![], |ui| {
                    checkbox(ui, &mut state, "Opção");
                });
                let boxes: Vec<_> = rects(&out)
                    .into_iter()
                    .filter(|r| r.rect.size() == vec2(14., 14.))
                    .collect();
                assert_eq!(boxes.len(), 1, "{theme:?} {on}");
                let tick = &boxes[0];
                let want = if on {
                    (p.acc, Stroke::NONE)
                } else {
                    (p.field, Stroke::new(1.0_f32, p.text2))
                };
                assert_eq!((tick.fill, tick.stroke), want, "{theme:?} {on}");
                // A cor que delimita a caixa: a borda, se houver; senão o fundo.
                let edge = if on { tick.fill } else { tick.stroke.color };
                for (name, back) in [("bg", p.bg), ("surf", p.surf), ("surf2", p.surf2)] {
                    let ratio = contrast(edge, back);
                    lowest = lowest.min(ratio);
                    assert!(ratio >= 3., "{theme:?} {on}: {ratio:.2}:1 on {name}");
                }
                if on {
                    let marks: Vec<Color32> = shapes(&out)
                        .into_iter()
                        .filter_map(|s| match s {
                            egui::Shape::Path(path) => match path.stroke.color {
                                egui::epaint::ColorMode::Solid(color) => Some(color),
                                egui::epaint::ColorMode::UV(_) => None,
                            },
                            _ => None,
                        })
                        .collect();
                    assert_eq!(marks, vec![p.on_acc], "{theme:?}: the tick");
                    let ratio = contrast(p.on_acc, p.acc);
                    assert!(ratio >= 3., "{theme:?}: tick on acc {ratio:.2}:1");
                }
            }
        }
        // O pior caso medido: marcada (acc) sobre surf2 no claro.
        assert!((lowest - 4.70).abs() < 0.01, "lowest {lowest:.2}:1");
        // O achado da Fase 3: a caixa do egui (surf2, borda line) some no
        // fundo escuro.
        let p = &theme::DARK;
        let ctx = context(egui::Theme::Dark);
        let out = run(&ctx, vec![], |ui| {
            ui.checkbox(&mut false, "Opção");
        });
        let at = rect_of(&node(&out, Role::CheckBox, "Opção"));
        let egui_box = rects(&out)
            .into_iter()
            .find(|r| at.contains_rect(r.rect) && r.stroke.width > 0.)
            .expect("the egui box");
        assert_eq!(egui_box.stroke.color, p.line, "precondition: egui's border");
        let ratio = contrast(egui_box.stroke.color, p.surf);
        assert!(ratio < 1.3, "egui's border on surf: {ratio:.2}:1");
    }

    /// Uma folha de exemplo; `typed` escolhe o nível.
    fn sheet_with(typed: Option<&str>, left: Duration) -> Confirm<'_> {
        Confirm {
            title: "Excluir o projeto?",
            subtitle: "Projeto: acme / legado",
            facts: &[],
            preview: "Prévia da alteração",
            warning: "Exclusão irreversível.",
            typed,
            verb: "Excluir",
            left,
            ready: true,
        }
    }
    const FIVE_MINUTES: Duration = Duration::from_secs(300);
    /// Dois frames: o primeiro de uma `Area` só mede (sizing pass).
    fn open(
        ctx: &egui::Context,
        size: [f32; 2],
        add: &mut impl FnMut(&mut Ui),
    ) -> egui::FullOutput {
        run_sized(ctx, size, vec![], &mut *add);
        run_sized(ctx, size, vec![], &mut *add)
    }

    #[test]
    fn the_sheet_hangs_from_the_top_of_the_content_area_over_a_veil() {
        for (theme, p) in THEMES {
            // (janela, largura da folha = min(540, largura − 32))
            for (size, width) in [([800., 600.], 540.), ([400., 600.], 368.)] {
                let ctx = context(theme);
                let mut input = String::new();
                let mut add = |ui: &mut Ui| {
                    confirm_sheet(ui, &sheet_with(None, FIVE_MINUTES), &mut input, |ui| {
                        ui.label("corpo da prévia");
                    });
                };
                let out = open(&ctx, size, &mut add);
                let case = format!("{theme:?} {size:?}");
                let screen = Rect::from_min_size(Pos2::ZERO, size.into());
                // Sem a sombra da folha, que no escuro tem a mesma cor.
                let veils: Vec<_> = rects(&out)
                    .into_iter()
                    .filter(|r| r.fill == VEIL && r.blur_width == 0.)
                    .map(|r| r.rect)
                    .collect();
                assert_eq!(veils, vec![screen], "{case}: the veil covers the panel");
                // O quadro em volta do título (o Cancelar também é surf/line).
                let title = rect_of(&node(&out, Role::Label, "Excluir o projeto?"));
                let frames: Vec<_> = rects(&out)
                    .into_iter()
                    .filter(|r| {
                        r.fill == p.surf
                            && r.stroke == Stroke::new(1.0_f32, p.line)
                            && r.rect.contains_rect(title)
                    })
                    .collect();
                assert_eq!(frames.len(), 1, "{case}");
                let frame = &frames[0];
                assert_eq!(frame.rect.top(), 0., "{case}: from the top");
                assert_eq!(frame.rect.width(), width, "{case}");
                assert_eq!(frame.rect.center().x, size[0] / 2., "{case}: centered");
                assert_eq!(
                    frame.corner_radius,
                    CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 12,
                        se: 12
                    },
                    "{case}: round only at the bottom"
                );
                for text in [
                    "Excluir o projeto?",
                    "Projeto: acme / legado",
                    "Prévia da alteração",
                    "corpo da prévia",
                    "Atenção. Exclusão irreversível.",
                    "Prévia válida por 5:00",
                ] {
                    assert!(
                        frame
                            .rect
                            .contains_rect(rect_of(&node(&out, Role::Label, text))),
                        "{case}: {text} inside the sheet"
                    );
                }
                let cancel = rect_of(&node(&out, Role::Button, "Cancelar"));
                let confirm = rect_of(&node(&out, Role::Button, "Excluir"));
                assert!(frame.rect.contains_rect(confirm), "{case}");
                assert!(
                    cancel.right() < confirm.left(),
                    "{case}: Cancelar, then the final button on the right"
                );
                assert_eq!((cancel.height(), confirm.height()), (28., 28.));
            }
        }
    }

    #[test]
    fn a_click_confirmation_enables_its_primary_button_without_typing() {
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let mut input = String::new();
            let mut answers = Vec::new();
            let mut add = |ui: &mut Ui| {
                if let Some(answer) =
                    confirm_sheet(ui, &sheet_with(None, FIVE_MINUTES), &mut input, |_| {})
                {
                    answers.push(answer);
                }
            };
            let out = open(&ctx, [800., 600.], &mut add);
            let button = node(&out, Role::Button, "Excluir");
            assert!(!button.is_disabled(), "{theme:?}");
            assert!(
                nodes(&out, Role::TextInput, "").is_empty(),
                "{theme:?}: no field to type in"
            );
            assert!(
                rects(&out)
                    .iter()
                    .any(|r| r.rect == rect_of(&button) && r.fill == p.acc),
                "{theme:?}: primary"
            );
            click_sized(&ctx, [800., 600.], rect_of(&button).center(), &mut add);
            drop(add);
            assert_eq!(answers, vec![Answer::Confirm], "{theme:?}");
        }
    }

    #[test]
    fn a_typed_confirmation_enables_only_on_the_exact_target() {
        let target = "acme / legado";
        // (texto no campo, pronto?, habilita?)
        for (typed, ready, enabled) in [
            ("", true, false),
            ("acme / leg", true, false),
            ("acme", true, false),
            (" acme / legado", true, false),
            ("acme / legado ", true, false),
            ("acme /  legado", true, false),
            ("ACME / LEGADO", true, false),
            ("CONFIRMAR acme / legado", true, false),
            ("acme / legado", false, false),
            ("acme / legado", true, true),
        ] {
            for (theme, p) in THEMES {
                let ctx = context(theme);
                let mut input = typed.to_owned();
                let mut answers = Vec::new();
                let mut add = |ui: &mut Ui| {
                    let sheet = Confirm {
                        ready,
                        ..sheet_with(Some(target), FIVE_MINUTES)
                    };
                    if let Some(answer) = confirm_sheet(ui, &sheet, &mut input, |_| {}) {
                        answers.push(answer);
                    }
                };
                let out = open(&ctx, [800., 600.], &mut add);
                let case = format!("{theme:?} {typed:?} ready {ready}");
                node(
                    &out,
                    Role::Label,
                    &format!("Digite {target} para confirmar"),
                );
                let button = node(&out, Role::Button, "Excluir");
                assert_eq!(button.is_disabled(), !enabled, "{case}");
                // Danger: bad, ou bad a 35 % desabilitado.
                let fill = rects(&out)
                    .into_iter()
                    .find(|r| r.rect == rect_of(&button))
                    .map(|r| r.fill);
                let want = if enabled {
                    p.bad
                } else {
                    p.bad.gamma_multiply(0.35)
                };
                assert!(
                    fill.is_some_and(|f| f
                        .to_array()
                        .iter()
                        .zip(want.to_array())
                        .all(|(a, b)| a.abs_diff(b) <= 1)),
                    "{case}: {fill:?}"
                );
                click_sized(&ctx, [800., 600.], rect_of(&button).center(), &mut add);
                drop(add);
                let want = if enabled {
                    vec![Answer::Confirm]
                } else {
                    vec![]
                };
                assert_eq!(answers, want, "{case}");
            }
        }
    }

    #[test]
    fn the_typed_field_takes_the_focus_so_typing_needs_no_click() {
        let ctx = context(egui::Theme::Dark);
        let mut input = String::new();
        let mut add = |ui: &mut Ui| {
            confirm_sheet(
                ui,
                &sheet_with(Some("demo"), FIVE_MINUTES),
                &mut input,
                |_| {},
            );
        };
        open(&ctx, [800., 600.], &mut add);
        let out = run(&ctx, vec![egui::Event::Text("demo".into())], &mut add);
        assert!(!node(&out, Role::Button, "Excluir").is_disabled());
        drop(add);
        assert_eq!(input, "demo");
    }

    #[test]
    fn a_paste_into_the_confirmation_field_stops_at_512_characters() {
        // Caracteres, não bytes: "é" tem 2 bytes.
        for (pasted, kept) in [(511, 511), (512, 512), (513, 512), (600, 512)] {
            let ctx = context(egui::Theme::Dark);
            let mut input = String::new();
            let target = "é".repeat(512);
            let mut add = |ui: &mut Ui| {
                confirm_sheet(
                    ui,
                    &sheet_with(Some(&target), FIVE_MINUTES),
                    &mut input,
                    |_| {},
                );
            };
            open(&ctx, [800., 600.], &mut add);
            let paste = egui::Event::Paste("é".repeat(pasted));
            let out = run(&ctx, vec![paste], &mut add);
            // O que cabe no limite é o alvo inteiro: habilita a partir de 512.
            assert_eq!(
                node(&out, Role::Button, "Excluir").is_disabled(),
                pasted < 512,
                "{pasted}"
            );
            drop(add);
            assert_eq!(input.chars().count(), kept, "{pasted}");
            assert_eq!(input.len(), 2 * kept, "{pasted}");
        }
    }

    #[test]
    fn the_countdown_shows_minutes_and_seconds_left_and_zero_closes_the_sheet() {
        for (left, text) in [
            (FIVE_MINUTES, Some("5:00")),
            (Duration::from_millis(299_500), Some("5:00")),
            (Duration::from_secs(299), Some("4:59")),
            (Duration::from_secs(61), Some("1:01")),
            (Duration::from_secs(60), Some("1:00")),
            (Duration::from_secs(1), Some("0:01")),
            (Duration::from_millis(1), Some("0:01")),
            (Duration::ZERO, None),
        ] {
            let ctx = context(egui::Theme::Light);
            let mut input = String::new();
            let mut answers = Vec::new();
            let mut add = |ui: &mut Ui| {
                answers.push(confirm_sheet(
                    ui,
                    &sheet_with(None, left),
                    &mut input,
                    |_| {},
                ));
            };
            open(&ctx, [800., 600.], &mut add);
            // Os dois primeiros frames da `Area` pedem repaint imediato; o
            // terceiro mostra o pedido da folha.
            let out = run(&ctx, vec![], &mut add);
            drop(add);
            let shown: Vec<String> = texts(&out)
                .into_iter()
                .filter_map(|(t, _)| t.strip_prefix("Prévia válida por ").map(str::to_owned))
                .collect();
            assert_eq!(
                shown,
                text.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                "{left:?}"
            );
            assert_eq!(
                nodes(&out, Role::Button, "Excluir").len(),
                usize::from(text.is_some()),
                "{left:?}"
            );
            assert!(answers.iter().all(Option::is_none), "{left:?}");
            // A folha aberta pede um frame por segundo; fechada, nenhum.
            let delay = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            if text.is_some() {
                assert!(delay <= Duration::from_secs(1), "{left:?}: {delay:?}");
            } else {
                assert_eq!(delay, Duration::MAX, "{left:?}");
            }
        }
    }

    #[test]
    fn cancel_and_escape_answer_cancel_unless_the_sheet_is_disabled() {
        for (way, enabled, want) in [
            ("button", true, vec![Answer::Cancel]),
            ("escape", true, vec![Answer::Cancel]),
            ("escape", false, vec![]),
        ] {
            let ctx = context(egui::Theme::Dark);
            let mut input = "acme".to_owned();
            let mut answers = Vec::new();
            let mut add = |ui: &mut Ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    if let Some(answer) = confirm_sheet(
                        ui,
                        &sheet_with(Some("acme"), FIVE_MINUTES),
                        &mut input,
                        |_| {},
                    ) {
                        answers.push(answer);
                    }
                });
            };
            let out = open(&ctx, [800., 600.], &mut add);
            let case = format!("{way} enabled {enabled}");
            for button in ["Cancelar", "Excluir"] {
                assert_eq!(
                    node(&out, Role::Button, button).is_disabled(),
                    !enabled,
                    "{case}: {button}"
                );
            }
            match way {
                "button" => {
                    let at = rect_of(&node(&out, Role::Button, "Cancelar")).center();
                    click_sized(&ctx, [800., 600.], at, &mut add);
                }
                _ => {
                    let key = |pressed| egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Default::default(),
                    };
                    run(&ctx, vec![key(true), key(false)], &mut add);
                }
            }
            drop(add);
            assert_eq!(answers, want, "{case}");
        }
    }

    #[test]
    fn the_veil_swallows_clicks_on_the_content_outside_the_sheet() {
        for open_sheet in [false, true] {
            let ctx = context(egui::Theme::Dark);
            let mut input = String::new();
            let mut below = 0;
            let mut add = |ui: &mut Ui| {
                // Um botão que ocupa a área toda, embaixo da folha.
                if ui
                    .add_sized(ui.available_size(), egui::Button::new("Embaixo"))
                    .clicked()
                {
                    below += 1;
                }
                if open_sheet {
                    confirm_sheet(ui, &sheet_with(None, FIVE_MINUTES), &mut input, |_| {});
                }
            };
            let out = open(&ctx, [800., 600.], &mut add);
            // Fora da folha: abaixo dela e no canto direito.
            for at in [pos2(400., 580.), pos2(790., 20.)] {
                if open_sheet {
                    let frame = rects(&out)
                        .into_iter()
                        .find(|r| r.corner_radius.se == 12 && r.corner_radius.nw == 0)
                        .expect("the sheet");
                    assert!(!frame.rect.contains(at), "precondition: {at:?} is outside");
                }
                click_sized(&ctx, [800., 600.], at, &mut add);
            }
            drop(add);
            assert_eq!(
                below,
                if open_sheet { 0 } else { 2 },
                "sheet open: {open_sheet}"
            );
        }
    }

    /// A caixa da prévia: o quadro de borda line e raio 8.
    fn preview_frame(out: &egui::FullOutput, p: &theme::Palette) -> Rect {
        let boxes: Vec<Rect> = rects(out)
            .into_iter()
            .filter(|r| {
                r.stroke == Stroke::new(1.0_f32, p.line) && r.corner_radius == CornerRadius::same(8)
            })
            .map(|r| r.rect)
            .collect();
        match boxes[..] {
            [one] => one,
            _ => panic!("one preview box, got {boxes:?}"),
        }
    }

    #[test]
    fn a_long_preview_scrolls_inside_a_box_of_bounded_height() {
        for (lines, tall) in [(2, false), (200, true)] {
            for (theme, p) in THEMES {
                let ctx = context(theme);
                let mut input = String::new();
                let out = open(&ctx, [800., 860.], &mut |ui: &mut Ui| {
                    confirm_sheet(ui, &sheet_with(None, FIVE_MINUTES), &mut input, |ui| {
                        for i in 0..lines {
                            ui.label(format!("linha {i}"));
                        }
                    });
                });
                let frame = preview_frame(&out, p);
                // Borda (2), cabeçalho (~23), margens (16) e até 240 de conteúdo.
                let case = format!("{theme:?} {lines} lines: {frame:?}");
                assert!(frame.height() <= 2. + 24. + 16. + PREVIEW_HEIGHT, "{case}");
                assert_eq!(frame.height() > PREVIEW_HEIGHT, tall, "{case}");
            }
        }
    }

    #[test]
    fn in_a_short_window_the_footer_stays_inside_the_content_area() {
        for height in [300., 420., 600.] {
            let ctx = context(egui::Theme::Dark);
            let mut input = String::new();
            let out = open(&ctx, [800., height], &mut |ui: &mut Ui| {
                confirm_sheet(
                    ui,
                    &sheet_with(Some("acme / legado"), FIVE_MINUTES),
                    &mut input,
                    |ui| {
                        for i in 0..200 {
                            ui.label(format!("linha {i}"));
                        }
                    },
                );
            });
            for button in ["Cancelar", "Excluir"] {
                let at = rect_of(&node(&out, Role::Button, button));
                assert!(
                    at.top() >= 0. && at.bottom() <= height - 16.,
                    "{height}: {button} at {at:?}"
                );
            }
            let countdown = rect_of(&node(&out, Role::Label, "Prévia válida por 5:00"));
            assert!(
                countdown.bottom() <= height - 16.,
                "{height}: {countdown:?}"
            );
        }
    }
}
