//! Página Sessões e handoffs (Sessions.dc.html): segmentado Handoffs ·
//! Sessões abertas · Projetos com a busca, linha de filtros, a lista de
//! handoffs agrupada por projeto com o detalhe e as tabelas de sessões e de
//! projetos. A página controla a própria rolagem (fora da rolagem da área
//! principal) e cabe na largura sem rolar para o lado. Só consulta: nenhum
//! handoff é aceito ou alterado aqui.
//!
//! Regras desta tela:
//! - Contadores do segmentado, sem filtro: Handoffs = total do servidor ("N+"
//!   sem o total, "—" antes da 1ª coleta); Sessões abertas = linhas sem
//!   encerramento na lista carregada (as que a aba mostra com "Só abertas" e
//!   sem filtros); Projetos = projetos listados ("—" antes da 1ª coleta).
//! - Grupos na ordem do handoff mais recente de cada projeto (a coleta já vem
//!   do mais novo ao mais velho); dentro do grupo, a mesma ordem.
//! - Título e prévia: `handoff_text::outline` do texto limpo; restrito =
//!   "Resumo restrito", sem resumo = "Sem resumo".
//! - Largura de conteúdo ≥ `SIDE_BY_SIDE`: lista (360–420) e detalhe lado a
//!   lado. Abaixo, um de cada vez: a lista; um clique abre o detalhe, com
//!   "← Voltar". Tudo sai da largura do frame; nada do layout anterior fica.
//! - Seleção = (projeto, ID). Abre no primeiro da lista filtrada e segue o ID
//!   entre coletas. Se o selecionado some (aceito em outro lugar) ou sai do
//!   filtro, passa ao primeiro da lista filtrada (nenhum, se vazia) e a tela
//!   estreita volta à lista: nunca o detalhe de um handoff fora da lista.
//! - Mudar busca, projeto, agente ou "Só abertas" rola a lista ao topo e, na
//!   tela estreita, volta à lista.
use crate::{
    app::{date, handoff_matches, handoff_warning_message, short_id, short_project},
    handoff_text::{self, Origin},
    model::{ApiSnapshot, Handoff, Project, Session, number},
    theme,
    widgets::{self, Button, Segment, Tone, Type, space},
};
use chrono::{DateTime, Datelike, Local, Utc};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Label, Layout, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Largura de conteúdo a partir da qual lista e detalhe ficam lado a lado:
/// 360 da lista + 1 da divisória + 420 do detalhe (as bases do artboard).
pub(crate) const SIDE_BY_SIDE: f32 = 781.;
const LIST_MIN: f32 = 360.;
const LIST_MAX: f32 = 420.;
const GROUP_HEIGHT: f32 = 28.;
const ROW_HEIGHT: f32 = 52.;
const LIST_FOOT: f32 = 34.;
const TABLE_HEAD: f32 = 28.;
const TABLE_ROW: f32 = 36.;
const CELL_PAD: f32 = 6.;
/// Altura do pé do detalhe: divisória, 12 de respiro e os botões.
const FOOT: f32 = 1. + 12. + 28.;
/// Abaixo desta altura útil o pé do detalhe rola junto com o texto.
const PINNED: f32 = 360.;
const FLEX_MIN: f32 = 120.;
/// Largura máxima de um filtro (projeto, agente).
const PICKER: f32 = 200.;
const SEARCH: &str = "Projeto, agente, ID ou resumo";
const READ_ONLY: &str = "Somente leitura · nenhum handoff é aceito por esta tela";
const LIMIT: &str = "Lista limitada a 200 por projeto";
const SESSIONS_NOTE: &str = "Últimas 100 sessões por projeto. Aberta = sem encerramento registrado; não comprova atividade atual. Vínculos entre projetos podem se repetir.";
const PROJECTS_NOTE: &str = "Atividade = último evento ou atualização informado pela API. Contagens por projeto podem compartilhar sessões.";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tab {
    Handoffs,
    Sessions,
    Projects,
}

pub(crate) struct Sessions {
    tab: Tab,
    search: String,
    /// "workspace / projeto"; vazio = todos.
    scope: String,
    /// Vazio = todos.
    agent: String,
    only_open: bool,
    /// (projeto, ID) do handoff no detalhe.
    selected: Option<(String, String)>,
    /// Tela estreita: o detalhe no lugar da lista.
    open: bool,
    /// O detalhe mostra o resumo bruto.
    raw: bool,
    /// Busca, projeto, agente e "Só abertas" do frame anterior.
    shown: (String, String, String, bool),
}
impl Default for Sessions {
    fn default() -> Self {
        Self {
            tab: Tab::Handoffs,
            search: String::new(),
            scope: String::new(),
            agent: String::new(),
            only_open: true,
            selected: None,
            open: false,
            raw: false,
            shown: (String::new(), String::new(), String::new(), true),
        }
    }
}

/// Uma linha da lista de handoffs, com a sua posição.
enum Item<'a> {
    Group(&'a str, usize),
    Row(&'a Handoff),
}
impl Item<'_> {
    fn height(&self) -> f32 {
        match self {
            Item::Group(..) => GROUP_HEIGHT,
            Item::Row(_) => ROW_HEIGHT,
        }
    }
}

struct Column {
    title: &'static str,
    /// `None` = a coluna que estica (o resto, com `FLEX_MIN` no mínimo).
    width: Option<f32>,
}
const SESSION_COLUMNS: [Column; 6] = [
    Column {
        title: "Estado",
        width: Some(96.),
    },
    Column {
        title: "Projeto / sessão",
        width: None,
    },
    Column {
        title: "Agente",
        width: Some(96.),
    },
    Column {
        title: "Eventos",
        width: Some(72.),
    },
    Column {
        title: "Início",
        width: Some(96.),
    },
    Column {
        title: "ID",
        width: Some(72.),
    },
];
const PROJECT_COLUMNS: [Column; 5] = [
    Column {
        title: "Projeto",
        width: None,
    },
    Column {
        title: "Páginas",
        width: Some(80.),
    },
    Column {
        title: "Abertas",
        width: Some(80.),
    },
    Column {
        title: "Eventos",
        width: Some(96.),
    },
    Column {
        title: "Atividade",
        width: Some(104.),
    },
];

impl Sessions {
    /// Reconexão: os filtros que dependem dos dados e a seleção.
    pub(crate) fn reset(&mut self) {
        self.scope.clear();
        self.agent.clear();
        self.selected = None;
        self.open = false;
        self.raw = false;
    }
    #[cfg(test)]
    pub(crate) fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }
    #[cfg(test)]
    pub(crate) fn set_filters(&mut self, scope: &str, agent: &str) {
        self.scope = scope.into();
        self.agent = agent.into();
    }
    #[cfg(test)]
    pub(crate) fn selected(&self) -> Option<&(String, String)> {
        self.selected.as_ref()
    }

    /// A página. Devolve `true` quando "Gerenciar em Revisão →" foi clicado.
    pub(crate) fn show(&mut self, ui: &mut Ui, api: &ApiSnapshot, now: DateTime<Utc>) -> bool {
        let collected = api.at.is_some();
        let total = api.handoff_total.map(number).unwrap_or_else(|| {
            if collected {
                format!("{}+", number(api.handoffs.len()))
            } else {
                "—".into()
            }
        });
        self.toolbar(ui, api, &total);
        self.filters(ui, api);
        let key = self.filter_key();
        let refilter = key != self.shown;
        self.shown = key;
        if refilter {
            self.open = false;
        }
        let needle = self.search.to_lowercase();
        match self.tab {
            Tab::Handoffs => {
                if !api.warnings.is_empty() {
                    let warning = handoff_warning_message(api.handoffs.len(), api.handoff_total);
                    widgets::strip(ui, Tone::Warn, &warning, "", None);
                }
                let shown: Vec<&Handoff> = api
                    .handoffs
                    .iter()
                    .filter(|h| {
                        handoff_matches(h, &self.scope, &needle)
                            && (self.agent.is_empty() || h.agent == self.agent)
                    })
                    .collect();
                let mut legend = Vec::new();
                if collected {
                    let projects = api
                        .handoffs
                        .iter()
                        .map(|h| h.scope.as_str())
                        .collect::<HashSet<_>>()
                        .len();
                    legend.push(format!(
                        "{total} {} em {} {} · visíveis para esta conexão",
                        if total == "1" {
                            "handoff aberto"
                        } else {
                            "handoffs abertos"
                        },
                        number(projects),
                        if projects == 1 { "projeto" } else { "projetos" }
                    ));
                }
                legend.push(LIMIT.into());
                split(ui, &legend, |ui| {
                    self.handoffs(ui, api, shown, &total, now, refilter)
                })
            }
            Tab::Sessions => {
                let rows: Vec<&Session> = api
                    .sessions
                    .iter()
                    .filter(|s| {
                        (!self.only_open || s.ended_at.is_none())
                            && (self.scope.is_empty() || s.scope == self.scope)
                            && (self.agent.is_empty() || s.agent_kind == self.agent)
                            && format!("{} {} {}", s.scope, s.agent_kind, s.session_id)
                                .to_lowercase()
                                .contains(&needle)
                    })
                    .collect();
                let legend = [
                    format!("{} registros", number(rows.len())),
                    SESSIONS_NOTE.into(),
                ];
                split(ui, &legend, |ui| sessions_table(ui, api, &rows, refilter));
                false
            }
            Tab::Projects => {
                let rows: Vec<&Project> = api
                    .projects
                    .iter()
                    .filter(|p| {
                        let scope = format!("{} / {}", p.workspace_name, p.project_name);
                        (self.scope.is_empty() || self.scope == scope)
                            && scope.to_lowercase().contains(&needle)
                    })
                    .collect();
                split(ui, &[PROJECTS_NOTE.into()], |ui| {
                    projects_table(ui, api, &rows, refilter)
                });
                false
            }
        }
    }
    fn filter_key(&self) -> (String, String, String, bool) {
        (
            self.search.clone(),
            self.scope.clone(),
            self.agent.clone(),
            self.only_open,
        )
    }

    fn toolbar(&mut self, ui: &mut Ui, api: &ApiSnapshot, total: &str) {
        let collected = api.at.is_some();
        let known = |n: usize| if collected { number(n) } else { "—".into() };
        let open = known(api.sessions.iter().filter(|s| s.ended_at.is_none()).count());
        let projects = known(api.projects.len());
        let mut below = false;
        let (tab, search) = (&mut self.tab, &mut self.search);
        widgets::toolbar(
            ui,
            |ui| {
                widgets::segmented(
                    ui,
                    tab,
                    &[
                        Segment {
                            value: Tab::Handoffs,
                            label: "Handoffs",
                            count: Some(total.to_owned()),
                        },
                        Segment {
                            value: Tab::Sessions,
                            label: "Sessões abertas",
                            count: Some(open),
                        },
                        Segment {
                            value: Tab::Projects,
                            label: "Projetos",
                            count: Some(projects),
                        },
                    ],
                );
            },
            |ui| {
                let width = ui.available_width().min(220.);
                if width >= 140. {
                    search_field(ui, search, width);
                } else {
                    below = true;
                }
            },
        );
        if below {
            let width = ui.available_width().min(320.);
            search_field(ui, &mut self.search, width);
        }
    }

    fn filters(&mut self, ui: &mut Ui, api: &ApiSnapshot) {
        let p = theme::palette(ui.visuals());
        let note =
            ui.painter()
                .layout_no_wrap(READ_ONLY.into(), FontId::proportional(12.), p.text2);
        let agents: Vec<String> = match self.tab {
            Tab::Handoffs => present(api.handoffs.iter().map(|h| h.agent.as_str()), &self.agent),
            Tab::Sessions => present(
                api.sessions.iter().map(|s| s.agent_kind.as_str()),
                &self.agent,
            ),
            Tab::Projects => Vec::new(),
        };
        let projects: Vec<String> = api
            .projects
            .iter()
            .map(|p| format!("{} / {}", p.workspace_name, p.project_name))
            .collect();
        let mut below = false;
        ui.horizontal(|ui| {
            ui.set_min_height(24.);
            ui.spacing_mut().item_spacing.x = 10.;
            ui.spacing_mut().button_padding = vec2(10., 4.);
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(12.));
            ui.visuals_mut().widgets.inactive.weak_bg_fill = p.surf;
            picker(
                ui,
                "sessions-scope",
                &mut self.scope,
                "Todos os projetos",
                "",
                &projects,
            );
            if self.tab != Tab::Projects {
                picker(
                    ui,
                    "sessions-agent",
                    &mut self.agent,
                    "Agente: todos",
                    "Agente: ",
                    &agents,
                );
            }
            if self.tab == Tab::Sessions {
                widgets::switch(ui, &mut self.only_open, "Só abertas");
            }
            if ui.available_width() >= note.size().x {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::caption(ui, READ_ONLY);
                });
            } else {
                below = true;
            }
        });
        if below {
            widgets::caption(ui, READ_ONLY);
        }
    }

    fn handoffs(
        &mut self,
        ui: &mut Ui,
        api: &ApiSnapshot,
        shown: Vec<&Handoff>,
        total: &str,
        now: DateTime<Utc>,
        refilter: bool,
    ) -> bool {
        let p = theme::palette(ui.visuals());
        let count = shown.len();
        let groups = group(shown);
        let ordered: Vec<&Handoff> = groups
            .iter()
            .flat_map(|(_, hs)| hs.iter().copied())
            .collect();
        let found = self.selected.as_ref().and_then(|(scope, id)| {
            ordered
                .iter()
                .copied()
                .find(|h| &h.scope == scope && &h.id == id)
        });
        let mut current = match found {
            Some(h) => Some(h),
            None => {
                // Sumiu ou saiu do filtro: o primeiro, e a lista na tela estreita.
                if self.selected.is_some() {
                    self.open = false;
                }
                self.raw = false;
                self.selected = ordered.first().map(|h| key(h));
                ordered.first().copied()
            }
        };
        let rect = ui.max_rect();
        if current.is_none() {
            empty(
                ui,
                rect,
                "Nenhum handoff aberto",
                if api.at.is_some() {
                    "Nenhum resultado visível para os filtros e as permissões atuais."
                } else {
                    "Aguardando a consulta ao ai-memory."
                },
            );
            return false;
        }
        let wide = rect.width() >= SIDE_BY_SIDE;
        if !wide && self.open {
            return current.is_some_and(|h| self.detail(ui, rect, h, now, true));
        }
        let list = if wide {
            let width = (rect.width() * 0.35).clamp(LIST_MIN, LIST_MAX);
            Rect::from_min_size(rect.min, vec2(width, rect.height()))
        } else {
            rect
        };
        if let Some(clicked) = self.list(ui, list, &groups, count, total, now, refilter) {
            if self.selected.as_ref() != Some(&clicked) {
                self.raw = false;
            }
            current = ordered
                .iter()
                .copied()
                .find(|h| h.scope == clicked.0 && h.id == clicked.1);
            self.selected = Some(clicked);
            self.open = true;
        }
        if !wide {
            return false;
        }
        ui.painter().vline(
            list.right() + 0.5,
            rect.y_range(),
            Stroke::new(1.0_f32, p.line),
        );
        let detail = Rect::from_min_max(pos2(list.right() + 1., rect.top()), rect.max);
        current.is_some_and(|h| self.detail(ui, detail, h, now, false))
    }

    /// A lista virtualizada: só os grupos e as linhas visíveis são montados.
    /// Devolve a linha clicada.
    #[allow(clippy::too_many_arguments)]
    fn list(
        &self,
        ui: &mut Ui,
        rect: Rect,
        groups: &[(&str, Vec<&Handoff>)],
        count: usize,
        total: &str,
        now: DateTime<Utc>,
        refilter: bool,
    ) -> Option<(String, String)> {
        let p = theme::palette(ui.visuals());
        pane(ui, rect, "Lista de handoffs");
        let mut items = Vec::new();
        let mut height = 0.;
        for (scope, rows) in groups {
            items.push((height, Item::Group(scope, rows.len())));
            height += GROUP_HEIGHT;
            for h in rows {
                items.push((height, Item::Row(h)));
                height += ROW_HEIGHT;
            }
        }
        let scroll = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - LIST_FOOT));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(scroll)
                .layout(Layout::top_down(Align::Min)),
        );
        let mut area = egui::ScrollArea::vertical()
            .id_salt("handoff-list")
            .auto_shrink(false);
        if refilter {
            area = area.vertical_scroll_offset(0.);
        }
        let mut clicked = None;
        area.show_viewport(&mut child, |ui, viewport| {
            ui.set_height(height);
            let content = ui.max_rect();
            let first = items.partition_point(|(top, item)| top + item.height() <= viewport.min.y);
            for (top, item) in &items[first..] {
                if *top >= viewport.max.y {
                    break;
                }
                let at = Rect::from_min_size(
                    pos2(content.left(), content.top() + top),
                    vec2(content.width(), item.height()),
                );
                match item {
                    Item::Group(scope, n) => group_head(ui, at, scope, *n),
                    Item::Row(h) => {
                        if self.row(ui, at, h, now).clicked() {
                            clicked = Some(key(h));
                        }
                    }
                }
            }
        });
        let foot = Rect::from_min_max(
            pos2(rect.left() + 16., scroll.bottom()),
            pos2(rect.right() - 16., rect.bottom()),
        );
        ui.new_child(
            UiBuilder::new()
                .max_rect(foot)
                .layout(Layout::left_to_right(Align::Center)),
        )
        .add(
            Label::new(
                RichText::new(format!("Mostrando {} de {total}", number(count)))
                    .size(12.)
                    .color(p.text2),
            )
            .truncate(),
        );
        clicked
    }

    fn row(&self, ui: &mut Ui, rect: Rect, h: &Handoff, now: DateTime<Utc>) -> Response {
        let p = theme::palette(ui.visuals());
        let selected = self
            .selected
            .as_ref()
            .is_some_and(|(scope, id)| *scope == h.scope && *id == h.id);
        let (title, preview) = headline(h);
        let age = age(&h.at, now).map_or_else(|| "—".to_owned(), |a| a.short());
        let agent = if h.agent.is_empty() { "—" } else { &h.agent };
        let response = ui.interact(
            rect,
            ui.id().with(("handoff", &h.scope, &h.id)),
            Sense::click(),
        );
        let spoken = format!("{title} · {agent} · {age}");
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, &spoken)
        });
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, 0., p.sel);
            painter.rect_filled(
                Rect::from_min_size(rect.min, vec2(3., rect.height())),
                0.,
                p.acc,
            );
        } else if response.hovered() {
            painter.rect_filled(rect, 0., widgets::tone(p.surf2, 0.6));
        }
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.line),
        );
        let (letter, fill, color) = avatar(&h.agent, p, ui.visuals().dark_mode);
        let badge = Rect::from_min_size(pos2(rect.left() + 16., rect.top() + 9.), Vec2::splat(24.));
        painter.rect_filled(badge, 6., fill);
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            letter,
            FontId::new(11., theme::semibold()),
            color,
        );
        let (left, right) = (badge.right() + 10., rect.right() - 16.);
        let age_galley = painter.layout_no_wrap(age, FontId::proportional(11.), p.text2);
        let second = if preview.is_empty() {
            agent.to_owned()
        } else {
            format!("{agent} · {preview}")
        };
        let title_galley = one_line(
            ui,
            &title,
            Type::Body.font(),
            p.text,
            right - left - age_galley.size().x - 8.,
        );
        let second_galley = one_line(ui, &second, Type::Caption.font(), p.text2, right - left);
        let elided = title_galley.elided || second_galley.elided;
        let top = rect.top() + 9.;
        let title_height = title_galley.size().y;
        painter.galley(
            pos2(
                right - age_galley.size().x,
                top + (title_height - age_galley.size().y) / 2.,
            ),
            age_galley,
            p.text2,
        );
        painter.galley(pos2(left, top), title_galley, p.text);
        painter.galley(pos2(left, top + title_height + 1.), second_galley, p.text2);
        if elided {
            response.on_hover_text(format!("{title}\n{second}"))
        } else {
            response
        }
    }

    /// O detalhe de `h` em `rect`. `back` = tela estreita: sem margem lateral
    /// e com "← Voltar". O pé (copiar e "Gerenciar em Revisão →") fica fixo
    /// embaixo quando sobra altura (`PINNED`); senão, vai ao fim do texto,
    /// que rola. Devolve `true` em "Gerenciar em Revisão →".
    fn detail(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        h: &Handoff,
        now: DateTime<Utc>,
        back: bool,
    ) -> bool {
        let p = theme::palette(ui.visuals());
        pane(ui, rect, "Detalhe do handoff");
        let inner = rect.shrink2(if back { vec2(0., 8.) } else { vec2(28., 22.) });
        let mut column = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        let ui = &mut column;
        if back && ui.add(Button::secondary("← Voltar").small()).clicked() {
            self.open = false;
        }
        let parsed = if h.redacted {
            None
        } else {
            h.summary.as_deref().map(handoff_text::parse)
        };
        let (title, _) = headline(h);
        let pinned = inner.height() >= PINNED;
        let free = ui.available_rect_before_wrap();
        let body = if pinned {
            Rect::from_min_max(
                free.min,
                pos2(free.right(), free.bottom() - FOOT - space::M),
            )
        } else {
            free
        };
        let mut scroll = ui.new_child(
            UiBuilder::new()
                .max_rect(body)
                .layout(Layout::top_down(Align::Min)),
        );
        let raw = &mut self.raw;
        // O texto que "Copiar resumo" leva: o da vista atual.
        let copy = |raw: bool| {
            parsed.as_ref().map(|s| {
                if raw && s.origin.is_some() {
                    s.raw.to_owned()
                } else {
                    s.body.to_string()
                }
            })
        };
        let mut review = egui::ScrollArea::vertical()
            .id_salt(("handoff-detail", &h.scope, &h.id))
            .auto_shrink(false)
            .show(&mut scroll, |ui| {
                ui.spacing_mut().item_spacing.y = 6.;
                ui.add(Label::new(RichText::new(&h.scope).size(12.).color(p.text2)).truncate());
                ui.add(Label::new(RichText::new(&title).font(Type::Detail.font())).wrap());
                meta(ui, h, now);
                ui.add_space(space::S);
                if let Some(origin) = parsed.as_ref().and_then(|s| s.origin.as_ref()) {
                    let (bold, text) = banner(origin);
                    let action = if *raw {
                        "Ver texto limpo"
                    } else {
                        "Ver texto bruto"
                    };
                    // No `scope`: o `strip` com ação devolve o cursor para
                    // dentro da faixa (o `put` do botão); o escopo o põe abaixo.
                    if ui
                        .scope(|ui| widgets::strip(ui, Tone::Info, bold, &text, Some(action)))
                        .inner
                        .is_some_and(|r| r.clicked())
                    {
                        *raw = !*raw;
                    }
                    ui.add_space(space::S);
                }
                // ~68ch: 68 algarismos do corpo.
                let reading = ui
                    .painter()
                    .layout_no_wrap("0".repeat(68), Type::Body.font(), p.text)
                    .size()
                    .x;
                ui.scope(|ui| {
                    ui.set_max_width(reading.min(ui.available_width()));
                    ui.spacing_mut().item_spacing.y = 10.;
                    match &parsed {
                        _ if h.redacted => {
                            body_text(ui, "Resumo restrito pelas permissões do servidor.", p.text2);
                        }
                        None => {
                            body_text(ui, "Sem resumo.", p.text2);
                        }
                        Some(s) if *raw && s.origin.is_some() => {
                            ui.add(Label::new(RichText::new(s.raw).font(Type::Mono.font())).wrap());
                        }
                        Some(s) => {
                            let rest = handoff_text::outline(&s.body).rest;
                            for paragraph in rest.split("\n\n") {
                                let paragraph = paragraph.trim_matches('\n');
                                if !paragraph.trim().is_empty() {
                                    body_text(ui, paragraph, p.text);
                                }
                            }
                        }
                    }
                    if let Some(steps) = h.next_steps.as_ref().filter(|s| !s.is_empty()) {
                        ui.add_space(space::XS);
                        ui.label(
                            RichText::new("Próximos passos")
                                .size(12.)
                                .family(theme::semibold())
                                .color(p.text2),
                        );
                        ui.spacing_mut().item_spacing.y = 4.;
                        for (i, step) in steps.iter().enumerate() {
                            body_text(ui, &format!("{}. {step}", i + 1), p.text);
                        }
                    }
                });
                if !pinned {
                    ui.add_space(space::M);
                    return footer(ui, h, copy(*raw));
                }
                false
            })
            .inner;
        if pinned {
            let mut foot = ui.new_child(
                UiBuilder::new()
                    .max_rect(Rect::from_min_max(
                        pos2(inner.left(), inner.bottom() - FOOT),
                        inner.max,
                    ))
                    .layout(Layout::top_down(Align::Min)),
            );
            review |= footer(&mut foot, h, copy(self.raw));
        }
        review
    }
}

/// Pé do detalhe: divisória, "Copiar resumo" (só com resumo legível),
/// "Copiar ID" e "Gerenciar em Revisão →" à direita. `FOOT` de altura.
/// Devolve `true` no link.
fn footer(ui: &mut Ui, h: &Handoff, copy: Option<String>) -> bool {
    let p = theme::palette(ui.visuals());
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.), Sense::hover());
    ui.painter().rect_filled(line, 0., p.line);
    ui.add_space((space::M - ui.spacing().item_spacing.y).max(0.));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S;
        if let Some(text) = copy
            && ui.add(Button::secondary("Copiar resumo")).clicked()
        {
            ui.ctx().copy_text(text);
        }
        if ui.add(Button::secondary("Copiar ID")).clicked() {
            ui.ctx().copy_text(h.id.clone());
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Com seleção de texto, o egui anuncia o link como Label.
            ui.style_mut().interaction.selectable_labels = false;
            ui.add(egui::Link::new(
                RichText::new("Gerenciar em Revisão →")
                    .size(12.)
                    .color(p.acc),
            ))
            .clicked()
        })
        .inner
    })
    .inner
}

/// Mede a legenda, dá à página o resto da altura e pinta a legenda embaixo.
fn split<R>(ui: &mut Ui, legend: &[String], content: impl FnOnce(&mut Ui) -> R) -> R {
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
        widgets::caption(ui, line);
    }
    inner
}

/// Os agentes presentes, em ordem, mais o filtro ativo se não estiver entre eles.
fn present<'a>(names: impl Iterator<Item = &'a str>, current: &str) -> Vec<String> {
    let mut agents: Vec<String> = names
        .filter(|n| !n.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_owned)
        .collect();
    if !current.is_empty() && !agents.iter().any(|a| a == current) {
        agents.push(current.to_owned());
    }
    agents
}

/// Filtro de 24 px: `all` (vazio) e as opções; o texto corta em `PICKER`.
fn picker(
    ui: &mut Ui,
    id: &str,
    current: &mut String,
    all: &str,
    prefix: &str,
    options: &[String],
) {
    let text = if current.is_empty() {
        all.to_owned()
    } else {
        format!("{prefix}{current}")
    };
    ui.allocate_ui_with_layout(
        vec2(PICKER, 24.),
        Layout::left_to_right(Align::Center),
        |ui| {
            // Largura sem corte: texto (12), seta e margens do botão.
            let spacing = ui.spacing();
            let natural = ui
                .painter()
                .layout_no_wrap(
                    text.clone(),
                    FontId::proportional(12.),
                    Color32::PLACEHOLDER,
                )
                .size()
                .x
                + spacing.icon_spacing
                + spacing.icon_width
                + 2. * spacing.button_padding.x;
            let response = egui::ComboBox::from_id_salt(id)
                .selected_text(text.clone())
                .truncate()
                .show_ui(ui, |ui| {
                    ui.selectable_value(current, String::new(), all);
                    for option in options {
                        ui.selectable_value(current, option.clone(), option);
                    }
                })
                .response;
            // Mais estreito que o texto: cortado, e o texto inteiro no tooltip.
            if response.rect.width() + 0.5 < natural {
                response.on_hover_text(text);
            }
        },
    );
}

fn search_field(ui: &mut Ui, text: &mut String, width: f32) {
    let p = theme::palette(ui.visuals());
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(SEARCH)
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
    widgets::strokes(
        ui.painter(),
        icon,
        &[&[(20., 20.), (16., 16.)]],
        2.,
        p.text2,
    );
}

fn key(h: &Handoff) -> (String, String) {
    (h.scope.clone(), h.id.clone())
}

/// Grupos por projeto, na ordem do primeiro (o mais recente) de cada um.
fn group(handoffs: Vec<&Handoff>) -> Vec<(&str, Vec<&Handoff>)> {
    let mut groups: Vec<(&str, Vec<&Handoff>)> = Vec::new();
    let mut index = HashMap::new();
    for h in handoffs {
        let at = *index.entry(h.scope.as_str()).or_insert_with(|| {
            groups.push((h.scope.as_str(), Vec::new()));
            groups.len() - 1
        });
        groups[at].1.push(h);
    }
    groups
}

/// Título e prévia de uma linha.
fn headline(h: &Handoff) -> (String, String) {
    if h.redacted {
        return ("Resumo restrito".into(), String::new());
    }
    let Some(summary) = h.summary.as_deref() else {
        return ("Sem resumo".into(), String::new());
    };
    let outline = handoff_text::outline(&handoff_text::parse(summary).body);
    if outline.title.is_empty() {
        ("Sem resumo".into(), String::new())
    } else {
        (outline.title, outline.preview)
    }
}

/// Inicial e cores do avatar: claude-code = "C" acc, codex = "X" info, outro
/// = a 1ª letra em neutro ("?" sem nome).
fn avatar(agent: &str, p: &theme::Palette, dark: bool) -> (String, Color32, Color32) {
    let alpha = if dark { 0.18 } else { 0.12 };
    match agent.trim() {
        a if a.eq_ignore_ascii_case("claude-code") => {
            ("C".into(), widgets::tone(p.acc, alpha), p.acc)
        }
        a if a.eq_ignore_ascii_case("codex") => ("X".into(), widgets::tone(p.info, alpha), p.info),
        a => (
            a.chars()
                .next()
                .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect()),
            p.surf2,
            p.text2,
        ),
    }
}

/// Idade de um handoff: a da lista ("12 min", "ontem") e a do detalhe.
#[derive(Clone, Debug, PartialEq)]
enum Age {
    Now,
    Minutes(i64),
    Hours(i64),
    Yesterday,
    Date(String),
}
impl Age {
    fn short(&self) -> String {
        match self {
            Age::Now => "agora".into(),
            Age::Minutes(n) => format!("{n} min"),
            Age::Hours(n) => format!("{n} h"),
            Age::Yesterday => "ontem".into(),
            Age::Date(d) => d.clone(),
        }
    }
    /// O parêntese depois da data no detalhe; a data já diz o resto.
    fn long(&self) -> Option<String> {
        match self {
            Age::Now => Some("agora".into()),
            Age::Minutes(n) => Some(format!("há {n} min")),
            Age::Hours(n) => Some(format!("há {n} h")),
            Age::Yesterday => Some("ontem".into()),
            Age::Date(_) => None,
        }
    }
}
/// Abaixo de 1 min = agora (o futuro, por relógio adiantado, também); até
/// 24 h, em min ou h; depois, "ontem" se foi no dia anterior (fuso local) e
/// senão a data ("dd/mm", com o ano se for outro). `None` = data ilegível.
fn age(at: &str, now: DateTime<Utc>) -> Option<Age> {
    let at = DateTime::parse_from_rfc3339(at).ok()?.with_timezone(&Utc);
    let secs = (now - at).num_seconds().max(0);
    Some(match secs {
        0..60 => Age::Now,
        60..3600 => Age::Minutes(secs / 60),
        3600..86_400 => Age::Hours(secs / 3600),
        _ => {
            let (at, now) = (at.with_timezone(&Local), now.with_timezone(&Local));
            if now.date_naive().pred_opt() == Some(at.date_naive()) {
                Age::Yesterday
            } else if at.year() == now.year() {
                Age::Date(at.format("%d/%m").to_string())
            } else {
                Age::Date(at.format("%d/%m/%y").to_string())
            }
        }
    })
}

/// ID longo como no artboard: começo…fim.
fn middle(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= 22 {
        return id.to_owned();
    }
    let head: String = chars[..13].iter().collect();
    let tail: String = chars[chars.len() - 7..].iter().collect();
    format!("{head}…{tail}")
}

/// Faixa de mensagem: o trecho em negrito e o resto. O nome vem do resumo e
/// é texto não confiável: só exibido, numa linha.
fn banner(origin: &Origin) -> (&'static str, String) {
    match origin {
        Origin::CrossSession { name, .. } => {
            let name = name
                .as_deref()
                .map(|n| {
                    n.split(|c: char| c.is_whitespace() || c.is_control())
                        .filter(|w| !w.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|n| !n.is_empty());
            (
                "Mensagem entre sessões",
                match name {
                    Some(name) => format!("· de “{name}” — marcação técnica ocultada."),
                    None => "— marcação técnica ocultada.".into(),
                },
            )
        }
        Origin::Subagent { .. } => ("Relato de subagente", "— marcação técnica ocultada.".into()),
    }
}

/// Pílula de estado (raio 10, texto 12).
fn pill(
    ui: &mut Ui,
    text: &str,
    fill: Color32,
    stroke: Option<Color32>,
    color: Color32,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(12.), color);
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(16., 4.), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    ui.painter().rect(
        rect,
        10.,
        fill,
        stroke.map_or(Stroke::NONE, |c| Stroke::new(1.0_f32, c)),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2., galley, color);
    response
}

/// "Aberto", agente, data (há …), ID curto e o botão de copiar o ID.
fn meta(ui: &mut Ui, h: &Handoff, now: DateTime<Utc>) {
    let p = theme::palette(ui.visuals());
    let small = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).size(12.).color(p.text2));
    };
    let when = match age(&h.at, now).and_then(|a| a.long()) {
        Some(ago) => format!("{} ({ago})", date(&h.at)),
        None => date(&h.at),
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10., 4.);
        pill(ui, "Aberto", p.surf2, Some(p.line), p.text);
        small(ui, if h.agent.is_empty() { "—" } else { &h.agent });
        small(ui, "·");
        small(ui, &when);
        small(ui, "·");
        ui.label(
            RichText::new(middle(&h.id))
                .font(Type::Mono.font())
                .color(p.text2),
        )
        .on_hover_text(&h.id);
        if copy_button(ui).clicked() {
            ui.ctx().copy_text(h.id.clone());
        }
    });
}

fn copy_button(ui: &mut Ui) -> Response {
    let p = theme::palette(ui.visuals());
    let (rect, response) = ui.allocate_exact_size(vec2(24., 22.), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), "Copiar ID do handoff")
    });
    let fill = if response.hovered() { p.surf2 } else { p.surf };
    ui.painter().rect(
        rect,
        5.,
        fill,
        Stroke::new(1.0_f32, p.line),
        StrokeKind::Inside,
    );
    widgets::strokes(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(12.)),
        &[
            &[(9., 9.), (20., 9.), (20., 20.), (9., 20.), (9., 9.)],
            &[(5., 15.), (4., 15.), (4., 4.), (15., 4.), (15., 5.)],
        ],
        2.,
        p.text2,
    );
    response.on_hover_text("Copiar ID")
}

/// Parágrafo do corpo: 13 px, entrelinha 20.
fn body_text(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(
        Label::new(
            RichText::new(text)
                .font(Type::Body.font())
                .color(color)
                .line_height(Some(20.)),
        )
        .wrap(),
    );
}

/// Texto numa linha, com reticências além de `width`.
fn one_line(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.));
    ui.painter().layout_job(job)
}

/// Nó AccessKit `Pane` com o rótulo do artboard (aria-label).
fn pane(ui: &mut Ui, rect: Rect, label: &str) {
    ui.interact(rect, ui.id().with(label), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Panel, true, label));
}

fn group_head(ui: &mut Ui, rect: Rect, scope: &str, n: usize) {
    let p = theme::palette(ui.visuals());
    let painter = ui.painter();
    painter.rect_filled(rect, 0., p.surf2);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0_f32, p.line),
    );
    let font = FontId::new(11., theme::semibold());
    let count = painter.layout_no_wrap(number(n), font.clone(), p.text2);
    let right = rect.right() - 16.;
    let name = one_line(
        ui,
        scope,
        font,
        p.text2,
        right - count.size().x - 8. - (rect.left() + 16.),
    );
    let elided = name.elided;
    let y = rect.center().y - name.size().y / 2.;
    ui.painter().galley(
        pos2(
            right - count.size().x,
            rect.center().y - count.size().y / 2.,
        ),
        count,
        p.text2,
    );
    ui.painter()
        .galley(pos2(rect.left() + 16., y), name, p.text2);
    let response = ui.interact(rect, ui.id().with(("group", scope)), Sense::hover());
    let spoken = format!("{scope} · {}", number(n));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &spoken));
    if elided {
        response.on_hover_text(scope);
    }
}

/// Mensagem no centro de uma área vazia.
fn empty(ui: &mut Ui, rect: Rect, title: &str, subtitle: &str) {
    let p = theme::palette(ui.visuals());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink(space::L))
            .layout(Layout::top_down(Align::Center)),
    );
    child.add_space(space::XL);
    child.label(RichText::new(title).font(Type::Section.font()));
    child.add(Label::new(RichText::new(subtitle).size(12.).color(p.text2)).wrap());
}

/// Cabeçalho (11 px, text2) e linhas de 36 px com divisória; só as linhas
/// visíveis são montadas. As larguras saem de `ui.max_rect()` a cada frame.
fn table(
    ui: &mut Ui,
    id: &str,
    columns: &[Column],
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
        let at = Rect::from_min_size(pos2(x, rect.top()), vec2(*width, TABLE_HEAD))
            .shrink2(vec2(CELL_PAD, 0.));
        ui.new_child(
            UiBuilder::new()
                .max_rect(at)
                .layout(Layout::left_to_right(Align::Center)),
        )
        .add(Label::new(RichText::new(column.title).size(11.).color(p.text2)).truncate());
        x += width;
    }
    ui.painter().hline(
        rect.x_range(),
        rect.top() + TABLE_HEAD - 0.5,
        Stroke::new(1.0_f32, p.line),
    );
    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + TABLE_HEAD), rect.max);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(body)
            .layout(Layout::top_down(Align::Min)),
    );
    let mut area = egui::ScrollArea::vertical().id_salt(id).auto_shrink(false);
    if top {
        area = area.vertical_scroll_offset(0.);
    }
    area.show_viewport(&mut child, |ui, viewport| {
        ui.set_height(TABLE_ROW * rows as f32);
        let content = ui.max_rect();
        let first = (viewport.min.y / TABLE_ROW).floor().max(0.) as usize;
        let last = ((viewport.max.y / TABLE_ROW).ceil().max(0.) as usize).min(rows);
        for row in first..last {
            let y = content.top() + row as f32 * TABLE_ROW;
            let mut x = content.left();
            for (column, width) in widths.iter().enumerate() {
                let at = Rect::from_min_size(pos2(x, y), vec2(*width, TABLE_ROW))
                    .shrink2(vec2(CELL_PAD, 0.));
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
                y + TABLE_ROW - 0.5,
                Stroke::new(1.0_f32, p.line),
            );
        }
    });
}

/// Tabela vazia: a mensagem embaixo do cabeçalho.
fn empty_table(ui: &mut Ui, api: &ApiSnapshot) {
    let rect = ui.max_rect();
    let below = Rect::from_min_max(pos2(rect.left(), rect.top() + TABLE_HEAD), rect.max);
    empty(
        ui,
        below,
        "Nenhum registro",
        if api.at.is_some() {
            "Nenhum resultado visível para os filtros e as permissões atuais."
        } else {
            "Aguardando a consulta ao ai-memory."
        },
    );
}

fn cell_text(ui: &mut Ui, text: &str) -> Response {
    ui.add(Label::new(RichText::new(text).size(13.)).truncate())
}

fn sessions_table(ui: &mut Ui, api: &ApiSnapshot, rows: &[&Session], top: bool) {
    let p = theme::palette(ui.visuals());
    // Projeto (13) sobre o ID (mono 11), centrados na linha de 36.
    let height = |font| {
        ui.painter()
            .layout_no_wrap("Ag".into(), font, p.text)
            .size()
            .y
    };
    let two_lines = height(FontId::proportional(13.)) + height(Type::Mono.font());
    table(
        ui,
        "sessions-table",
        &SESSION_COLUMNS,
        rows.len(),
        top,
        |ui, row, column| {
            let s = rows[row];
            match column {
                0 => {
                    if s.ended_at.is_none() {
                        pill(ui, "Aberta", p.sel, None, p.acc);
                    } else {
                        pill(ui, "Encerrada", p.surf2, Some(p.line), p.text2);
                    }
                }
                1 => {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.;
                        ui.add_space(((TABLE_ROW - two_lines) / 2.).max(0.));
                        cell_text(ui, short_project(&s.scope)).on_hover_text(&s.scope);
                        ui.add(
                            Label::new(
                                RichText::new(short_id(&s.session_id))
                                    .font(Type::Mono.font())
                                    .color(p.text2),
                            )
                            .truncate(),
                        )
                        .on_hover_text(s.cwd.as_deref().unwrap_or(""));
                    });
                }
                2 => {
                    cell_text(ui, &s.agent_kind);
                }
                3 => {
                    cell_text(ui, &number(s.observation_count as usize));
                }
                4 => {
                    cell_text(ui, &date(&s.started_at));
                }
                _ => {
                    if ui.add(Button::secondary("Copiar").small()).clicked() {
                        ui.ctx().copy_text(s.session_id.clone());
                    }
                }
            }
        },
    );
    if rows.is_empty() {
        empty_table(ui, api);
    }
}

fn projects_table(ui: &mut Ui, api: &ApiSnapshot, rows: &[&Project], top: bool) {
    let dash = |n: Option<u64>| n.map_or_else(|| "—".to_owned(), |n| number(n as usize));
    table(
        ui,
        "projects-table",
        &PROJECT_COLUMNS,
        rows.len(),
        top,
        |ui, row, column| {
            let project = rows[row];
            match column {
                0 => {
                    cell_text(ui, &project.project_name).on_hover_text(&project.workspace_name);
                }
                1 => {
                    cell_text(ui, &number(project.page_count as usize));
                }
                2 => {
                    cell_text(ui, &dash(project.open_session_count));
                }
                3 => {
                    cell_text(ui, &dash(project.observation_count));
                }
                _ => {
                    cell_text(
                        ui,
                        &project
                            .last_activity
                            .as_deref()
                            .map_or_else(|| "—".to_owned(), date),
                    );
                }
            }
        },
    );
    if rows.is_empty() {
        empty_table(ui, api);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::tests::{THEMES, context, node, nodes, rect_of, rects, run_sized, texts};
    use chrono::TimeZone;
    use egui::{
        Event, Pos2,
        accesskit::{Role, Toggled},
    };

    /// 1100 − 16 de margem = 1084 de conteúdo: lado a lado.
    const WIDE: [f32; 2] = [1100., 800.];
    /// 610 − 16 = 594, o conteúdo de 720 no macOS: um de cada vez.
    const NARROW: [f32; 2] = [610., 600.];

    fn at(now: DateTime<Utc>, minutes: i64) -> String {
        (now - chrono::Duration::minutes(minutes)).to_rfc3339()
    }
    fn handoff(
        now: DateTime<Utc>,
        scope: &str,
        id: &str,
        agent: &str,
        minutes: i64,
        summary: &str,
    ) -> Handoff {
        Handoff {
            id: id.into(),
            agent: agent.into(),
            at: at(now, minutes),
            state: "open".into(),
            summary: Some(summary.into()),
            scope: scope.into(),
            ..Default::default()
        }
    }
    fn collected(handoffs: Vec<Handoff>) -> ApiSnapshot {
        ApiSnapshot {
            at: Some(Utc::now()),
            handoff_total: Some(handoffs.len()),
            handoffs,
            ..Default::default()
        }
    }
    /// Três handoffs de um projeto: A (1 min), B (5 min), C (10 min, codex).
    fn abc(now: DateTime<Utc>) -> Vec<Handoff> {
        vec![
            handoff(now, "ws / p1", "id-a", "claude-code", 1, "## A\n\ncorpo A"),
            handoff(now, "ws / p1", "id-b", "claude-code", 5, "## B\n\ncorpo B"),
            handoff(now, "ws / p1", "id-c", "codex", 10, "## C\n\ncorpo C"),
        ]
    }
    const CROSS: &str = "Started: <cross-session-message from=\"local_0001\" from-name=\"Trilha &quot;A&quot; &amp;&#10;B\">\nPergunta ao dono.\n\nSegundo parágrafo.\nCom linha.\n</cross-session-message>";
    const CROSS_BODY: &str = "Pergunta ao dono.\n\nSegundo parágrafo.\nCom linha.";
    const AGENT: &str = "<agent-message from=\"a1\">\n[Subagent hand-back] The text below is the final report. The report follows:\n  ## Lote pronto\n  \n  Três casos.";

    /// A página num contexto, com os dados e o relógio fixos.
    struct Page {
        ctx: egui::Context,
        size: [f32; 2],
        sessions: Sessions,
        api: ApiSnapshot,
        now: DateTime<Utc>,
        review: usize,
    }
    impl Page {
        fn new(theme: egui::Theme, size: [f32; 2], api: ApiSnapshot, now: DateTime<Utc>) -> Self {
            Self {
                ctx: context(theme),
                size,
                sessions: Sessions::default(),
                api,
                now,
                review: 0,
            }
        }
        fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
            let Self {
                ctx,
                size,
                sessions,
                api,
                now,
                review,
            } = self;
            run_sized(ctx, *size, events, |ui| {
                if sessions.show(ui, api, *now) {
                    *review += 1;
                }
            })
        }
        fn show(&mut self) -> egui::FullOutput {
            self.frame(vec![])
        }
        /// Clique completo; devolve o frame em que ele acontece (o de soltar).
        fn click(&mut self, pos: Pos2) -> egui::FullOutput {
            let press = |pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            self.frame(vec![Event::PointerMoved(pos), press(true)]);
            self.frame(vec![press(false)])
        }
        fn click_on(&mut self, role: Role, text: &str) -> egui::FullOutput {
            let out = self.show();
            self.click(rect_of(&node(&out, role, text)).center())
        }
        /// Roda a roda sobre `pos` e espera a animação; devolve o último frame.
        fn wheel(&mut self, pos: Pos2, dy: f32) -> egui::FullOutput {
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![
                Event::PointerMoved(pos),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0., dy),
                    modifiers: Default::default(),
                },
            ]);
            let mut out = self.show();
            for _ in 0..30 {
                out = self.frame(vec![Event::PointerMoved(pos)]);
            }
            out
        }
    }
    /// Linhas da lista, de cima para baixo: (rótulo, selecionada).
    fn rows(out: &egui::FullOutput) -> Vec<(String, bool)> {
        let mut found: Vec<(f64, String, bool)> = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.role() == Role::Button && n.label().is_some_and(|l| l.matches(" · ").count() >= 2)
            })
            .map(|(_, n)| {
                (
                    n.bounds().unwrap().y0,
                    n.label().unwrap().to_owned(),
                    n.toggled() == Some(Toggled::True),
                )
            })
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|(_, l, s)| (l, s)).collect()
    }
    fn selected(out: &egui::FullOutput) -> Vec<String> {
        rows(out)
            .into_iter()
            .filter(|(_, s)| *s)
            .map(|(l, _)| l)
            .collect()
    }
    fn copied(out: &egui::FullOutput) -> Vec<String> {
        out.platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
    fn has(out: &egui::FullOutput, role: Role, text: &str) -> bool {
        !nodes(out, role, text).is_empty()
    }

    #[test]
    fn segment_counts_for_every_collection_state() {
        let now = Utc::now();
        let session = |id: &str, scope: &str, ended: bool| Session {
            session_id: id.into(),
            scope: scope.into(),
            agent_kind: "codex".into(),
            started_at: at(now, 5),
            ended_at: ended.then(|| at(now, 1)),
            ..Default::default()
        };
        let sessions = vec![
            session("s-a", "ws / p1", false),
            session("s-a", "ws / p2", false),
            session("s-b", "ws / p1", true),
            session("s-c", "ws / p1", false),
        ];
        let projects = vec![Project::default(); 14];
        let listed = |n: usize| vec![Handoff::default(); n];
        // (caso, coleta?, total do servidor, listados, [Handoffs, Sessões abertas, Projetos])
        for (case, at, total, handoffs, want) in [
            ("no collection", None, None, 3, ["—", "—", "—"]),
            (
                "server total",
                Some(now),
                Some(1234),
                3,
                ["1.234", "3", "14"],
            ),
            (
                "list only, may be cut",
                Some(now),
                None,
                200,
                ["200+", "3", "14"],
            ),
            (
                "total before the collection flag",
                None,
                Some(5),
                0,
                ["5", "—", "—"],
            ),
        ] {
            let api = ApiSnapshot {
                at,
                handoff_total: total,
                handoffs: listed(handoffs),
                sessions: sessions.clone(),
                projects: projects.clone(),
                ..Default::default()
            };
            let mut page = Page::new(egui::Theme::Dark, WIDE, api, now);
            let out = page.show();
            for (label, count) in ["Handoffs", "Sessões abertas", "Projetos"].iter().zip(want) {
                node(&out, Role::Button, &format!("{label} {count}"));
            }
            assert_eq!(
                node(&out, Role::Button, &format!("Handoffs {}", want[0])).toggled(),
                Some(Toggled::True),
                "{case}: opens on Handoffs"
            );
        }
        // Coletado e vazio: zeros, não "—".
        let mut page = Page::new(egui::Theme::Light, WIDE, collected(vec![]), now);
        let out = page.show();
        for text in ["Handoffs 0", "Sessões abertas 0", "Projetos 0"] {
            node(&out, Role::Button, text);
        }
    }

    #[test]
    fn each_tab_shows_its_own_content_in_both_themes() {
        let now = Utc::now();
        for (theme, _) in THEMES {
            let mut api = collected(abc(now));
            api.projects = vec![Project::default(); 2];
            let mut page = Page::new(theme, WIDE, api, now);
            for (segment, tab) in [
                ("Sessões abertas 0", Tab::Sessions),
                ("Projetos 2", Tab::Projects),
                ("Handoffs 3", Tab::Handoffs),
            ] {
                page.click_on(Role::Button, segment);
                let out = page.show();
                assert_eq!(page.sessions.tab, tab, "{theme:?}");
                assert_eq!(
                    node(&out, Role::Button, segment).toggled(),
                    Some(Toggled::True),
                    "{theme:?} {segment}"
                );
                node(&out, Role::Label, READ_ONLY);
                node(&out, Role::ComboBox, "Todos os projetos");
                let (list, switch, sessions, projects, agent) = (
                    has(&out, Role::Pane, "Lista de handoffs"),
                    has(&out, Role::CheckBox, "Só abertas"),
                    has(&out, Role::Label, "Projeto / sessão"),
                    has(&out, Role::Label, "Atividade"),
                    has(&out, Role::ComboBox, "Agente: todos"),
                );
                let want = match tab {
                    Tab::Handoffs => (true, false, false, false, true),
                    Tab::Sessions => (false, true, true, false, true),
                    Tab::Projects => (false, false, false, true, false),
                };
                assert_eq!(
                    (list, switch, sessions, projects, agent),
                    want,
                    "{theme:?} {tab:?}"
                );
            }
        }
    }

    #[test]
    fn groups_follow_recency_with_counts_and_rows_keep_the_collection_order() {
        let now = Utc::now();
        let api = collected(vec![
            handoff(now, "ws / p1", "a", "claude-code", 1, "Alfa"),
            handoff(now, "ws / p2", "b", "claude-code", 5, "Beta"),
            handoff(now, "ws / p1", "c", "claude-code", 10, "Gama"),
            handoff(now, "ws / p3", "d", "codex", 60, "Delta"),
            handoff(now, "ws / p2", "e", "codex", 120, "Épsilon"),
        ]);
        let mut page = Page::new(egui::Theme::Dark, WIDE, api, now);
        let out = page.show();
        assert_eq!(
            rows(&out).into_iter().map(|(l, _)| l).collect::<Vec<_>>(),
            [
                "Alfa · claude-code · 1 min",
                "Gama · claude-code · 10 min",
                "Beta · claude-code · 5 min",
                "Épsilon · codex · 2 h",
                "Delta · codex · 1 h",
            ]
        );
        let y = |role, text| rect_of(&node(&out, role, text)).top();
        let order = [
            y(Role::Label, "ws / p1 · 2"),
            y(Role::Button, "Alfa · claude-code · 1 min"),
            y(Role::Label, "ws / p2 · 2"),
            y(Role::Button, "Beta · claude-code · 5 min"),
            y(Role::Label, "ws / p3 · 1"),
            y(Role::Button, "Delta · codex · 1 h"),
        ];
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "each group heads its rows: {order:?}"
        );
        assert_eq!(
            y(Role::Button, "Alfa · claude-code · 1 min") - y(Role::Label, "ws / p1 · 2"),
            GROUP_HEIGHT
        );
        assert_eq!(
            y(Role::Button, "Gama · claude-code · 10 min")
                - y(Role::Button, "Alfa · claude-code · 1 min"),
            ROW_HEIGHT
        );
        node(&out, Role::Label, "Mostrando 5 de 5");
    }

    #[test]
    fn rows_show_avatar_title_age_and_agent_preview_in_both_themes() {
        let now = Utc::now();
        for (theme, p) in THEMES {
            let alpha = if theme == egui::Theme::Dark {
                0.18
            } else {
                0.12
            };
            let api = collected(vec![
                handoff(
                    now,
                    "ws / p",
                    "1",
                    "claude-code",
                    12,
                    "## **Lote** pronto\n\nTrês casos.",
                ),
                handoff(now, "ws / p", "2", "codex", 60, "Revisar índices"),
                handoff(now, "ws / p", "3", "gemini", 61, "Outro agente"),
                handoff(now, "ws / p", "4", "", 62, "Sem agente"),
                handoff(now, "ws / p", "5", "Claude-Code", 63, "Maiúsculas"),
            ]);
            let mut page = Page::new(theme, WIDE, api, now);
            let out = page.show();
            assert_eq!(
                rows(&out),
                [
                    ("Lote pronto · claude-code · 12 min".to_owned(), true),
                    ("Revisar índices · codex · 1 h".to_owned(), false),
                    ("Outro agente · gemini · 1 h".to_owned(), false),
                    ("Sem agente · — · 1 h".to_owned(), false),
                    ("Maiúsculas · Claude-Code · 1 h".to_owned(), false),
                ],
                "{theme:?}"
            );
            let painted = texts(&out);
            for (text, color) in [
                ("C", p.acc),
                ("X", p.info),
                ("G", p.text2),
                ("?", p.text2),
                ("Lote pronto", p.text),
                ("12 min", p.text2),
                ("claude-code · Três casos.", p.text2),
                ("codex", p.text2),
                ("—", p.text2),
            ] {
                assert!(
                    painted.contains(&(text.to_owned(), color)),
                    "{theme:?}: {text} in {color:?}"
                );
            }
            assert_eq!(
                painted.iter().filter(|(t, _)| t == "C").count(),
                2,
                "claude-code in any case is C"
            );
            let avatars: Vec<Color32> = rects(&out)
                .into_iter()
                .filter(|r| r.rect.size() == vec2(24., 24.) && r.corner_radius == 6.into())
                .map(|r| r.fill)
                .collect();
            assert_eq!(
                avatars,
                [
                    widgets::tone(p.acc, alpha),
                    widgets::tone(p.info, alpha),
                    p.surf2,
                    p.surf2,
                    widgets::tone(p.acc, alpha),
                ],
                "{theme:?}"
            );
            // Selecionada: fundo sel na linha toda e o filete acc de 3 px.
            let row = rect_of(&node(
                &out,
                Role::Button,
                "Lote pronto · claude-code · 12 min",
            ));
            let painted = rects(&out);
            assert!(
                painted.iter().any(|r| r.rect == row && r.fill == p.sel),
                "{theme:?}: selected fill"
            );
            assert!(
                painted.iter().any(|r| r.fill == p.acc
                    && r.rect == Rect::from_min_size(row.min, vec2(3., row.height()))),
                "{theme:?}: selected edge"
            );
            let other = rect_of(&node(&out, Role::Button, "Revisar índices · codex · 1 h"));
            assert!(
                !painted.iter().any(|r| r.rect == other && r.fill == p.sel),
                "{theme:?}: only the selected row is filled"
            );
        }
    }

    #[test]
    fn ages_for_every_range() {
        let now = Local
            .with_ymd_and_hms(2026, 3, 18, 12, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let ago = |secs: i64| (now - chrono::Duration::seconds(secs)).to_rfc3339();
        let local = |y, mo, d, h, mi| {
            Local
                .with_ymd_and_hms(y, mo, d, h, mi, 0)
                .single()
                .unwrap()
                .to_rfc3339()
        };
        let in_tokyo = (now - chrono::Duration::hours(2))
            .with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap())
            .to_rfc3339();
        for (case, at, want) in [
            ("garbage", "ontem".to_owned(), None),
            ("empty", String::new(), None),
            ("future (clock ahead)", ago(-300), Some(Age::Now)),
            ("0 s", ago(0), Some(Age::Now)),
            ("59 s", ago(59), Some(Age::Now)),
            ("60 s", ago(60), Some(Age::Minutes(1))),
            ("3599 s", ago(3599), Some(Age::Minutes(59))),
            ("3600 s", ago(3600), Some(Age::Hours(1))),
            ("23:59:59", ago(86_399), Some(Age::Hours(23))),
            ("24 h, the day before", ago(86_400), Some(Age::Yesterday)),
            (
                "early yesterday",
                local(2026, 3, 17, 0, 30),
                Some(Age::Yesterday),
            ),
            (
                "two days ago, late",
                local(2026, 3, 16, 23, 30),
                Some(Age::Date("16/03".into())),
            ),
            (
                "another year",
                local(2025, 12, 31, 10, 0),
                Some(Age::Date("31/12/25".into())),
            ),
            ("other offset", in_tokyo, Some(Age::Hours(2))),
        ] {
            assert_eq!(age(&at, now), want, "{case}");
        }
        for (a, short, long) in [
            (Age::Now, "agora", Some("agora")),
            (Age::Minutes(12), "12 min", Some("há 12 min")),
            (Age::Hours(3), "3 h", Some("há 3 h")),
            (Age::Yesterday, "ontem", Some("ontem")),
            (Age::Date("16/03".into()), "16/03", None),
        ] {
            assert_eq!(a.short(), short);
            assert_eq!(a.long().as_deref(), long, "{a:?}");
        }
    }

    #[test]
    fn selection_opens_on_the_first_and_follows_the_id_between_collections() {
        let now = Utc::now();
        let mut page = Page::new(egui::Theme::Dark, WIDE, collected(abc(now)), now);
        let out = page.show();
        assert_eq!(selected(&out), ["A · claude-code · 1 min"]);
        node(&out, Role::Label, "A");
        node(&out, Role::Label, "corpo A");
        page.click_on(Role::Button, "B · claude-code · 5 min");
        let out = page.show();
        assert_eq!(selected(&out), ["B · claude-code · 5 min"]);
        node(&out, Role::Label, "B");
        assert!(!has(&out, Role::Label, "A"), "one detail at a time");
        // Coleta nova: um handoff mais novo no topo e o B com texto novo.
        let mut next = abc(now);
        next[1].summary = Some("## B\n\ncorpo B revisado".into());
        next.insert(
            0,
            handoff(now, "ws / p0", "id-z", "codex", 0, "## Z\n\nnovo"),
        );
        page.api = collected(next);
        let out = page.show();
        assert_eq!(
            selected(&out),
            ["B · claude-code · 5 min"],
            "the selection follows the ID, not the position"
        );
        node(&out, Role::Label, "corpo B revisado");
        assert!(!has(&out, Role::Label, "corpo B"));
        assert_eq!(rows(&out)[0], ("Z · codex · agora".to_owned(), false));
        // O mesmo ID em outro projeto é outro handoff.
        let mut twin = abc(now);
        twin[0].id = "id-b".into();
        twin[0].scope = "ws / outro".into();
        twin.retain(|h| h.scope != "ws / p1" || h.id != "id-b");
        page.api = collected(twin);
        let out = page.show();
        assert_eq!(
            selected(&out),
            ["A · claude-code · 1 min"],
            "(project, ID) is the key: B of ws / p1 is gone"
        );
    }

    #[test]
    fn a_selected_handoff_that_leaves_the_list_moves_to_the_first_and_never_stays_in_the_detail() {
        let now = Utc::now();
        // Some (aceito em outro lugar).
        let mut page = Page::new(egui::Theme::Light, WIDE, collected(abc(now)), now);
        page.click_on(Role::Button, "C · codex · 10 min");
        node(&page.show(), Role::Label, "C");
        page.api = collected(abc(now)[..2].to_vec());
        let out = page.show();
        assert_eq!(selected(&out), ["A · claude-code · 1 min"]);
        node(&out, Role::Label, "A");
        assert!(!has(&out, Role::Label, "C"));
        // Sai do filtro, com a lista ainda cheia: o primeiro que sobra.
        page.api = collected(abc(now));
        page.click_on(Role::Button, "B · claude-code · 5 min");
        page.sessions.agent = "codex".into();
        let out = page.show();
        assert_eq!(rows(&out), [("C · codex · 10 min".to_owned(), true)]);
        node(&out, Role::Label, "C");
        // O filtro esvazia a lista: nenhum detalhe; ao limpar, o primeiro.
        page.sessions.agent.clear();
        page.sessions.search = "nada assim".into();
        let out = page.show();
        node(&out, Role::Label, "Nenhum handoff aberto");
        node(
            &out,
            Role::Label,
            "Nenhum resultado visível para os filtros e as permissões atuais.",
        );
        assert!(!has(&out, Role::Pane, "Detalhe do handoff"));
        assert!(rows(&out).is_empty());
        page.sessions.search.clear();
        assert_eq!(selected(&page.show()), ["A · claude-code · 1 min"]);
        // Estreita, com o detalhe aberto: o handoff some e a tela volta à lista.
        let mut page = Page::new(egui::Theme::Dark, NARROW, collected(abc(now)), now);
        page.click_on(Role::Button, "B · claude-code · 5 min");
        let out = page.show();
        node(&out, Role::Pane, "Detalhe do handoff");
        node(&out, Role::Label, "B");
        page.api = collected(vec![abc(now)[0].clone(), abc(now)[2].clone()]);
        let out = page.show();
        node(&out, Role::Pane, "Lista de handoffs");
        assert!(!has(&out, Role::Pane, "Detalhe do handoff"));
        assert!(!has(&out, Role::Label, "B") && !has(&out, Role::Label, "A"));
        assert_eq!(selected(&out), ["A · claude-code · 1 min"]);
        // Mudar o filtro na tela estreita também volta à lista.
        page.click_on(Role::Button, "C · codex · 10 min");
        node(&page.show(), Role::Pane, "Detalhe do handoff");
        page.sessions.search = "c".into();
        let out = page.show();
        node(&out, Role::Pane, "Lista de handoffs");
        assert!(!has(&out, Role::Pane, "Detalhe do handoff"));
    }

    /// 200 handoffs num projeto: "Item 0" (1 min) … "Item 199".
    fn two_hundred(now: DateTime<Utc>) -> Vec<Handoff> {
        (0..200)
            .map(|i| {
                handoff(
                    now,
                    "ws / grande",
                    &format!("id-{i}"),
                    "claude-code",
                    i + 1,
                    &format!("Item {i}\nprévia"),
                )
            })
            .collect()
    }

    #[test]
    fn changing_a_filter_with_the_list_scrolled_goes_back_to_the_top() {
        let now = Utc::now();
        let mut page = Page::new(egui::Theme::Dark, WIDE, collected(two_hundred(now)), now);
        let out = page.show();
        let list = rect_of(&node(&out, Role::Pane, "Lista de handoffs"));
        let first = "Item 0 · claude-code · 1 min";
        assert!(has(&out, Role::Button, first));
        let out = page.wheel(list.center(), -3000.);
        assert!(
            !has(&out, Role::Button, first),
            "scrolled away from the top"
        );
        let out = page.show();
        assert!(
            !has(&out, Role::Button, first),
            "with no filter change the list stays where it was"
        );
        // A busca tira o selecionado (Item 0): a lista volta ao topo, no 1º.
        page.sessions.search = "item 1".into();
        let out = page.show();
        let match_one = "Item 1 · claude-code · 2 min";
        let row = rect_of(&node(&out, Role::Button, match_one));
        assert!(
            list.contains_rect(row) && row.top() - list.top() <= GROUP_HEIGHT + 0.5,
            "the first match is the first row: {row:?} in {list:?}"
        );
        assert_eq!(selected(&out), [match_one]);
        node(&out, Role::Label, "Mostrando 111 de 200");
        // Sem outra mudança, a lista filtrada rola e fica onde parou.
        let out = page.wheel(list.center(), -3000.);
        assert!(
            !has(&out, Role::Button, match_one),
            "scrolled the filtered list"
        );
        assert!(!has(&page.show(), Role::Button, match_one));
    }

    #[test]
    fn only_the_visible_rows_are_mounted() {
        let now = Utc::now();
        let handoffs: Vec<Handoff> = (0..15)
            .flat_map(|p| {
                (0..200).map(move |i| {
                    handoff(
                        now,
                        &format!("ws / p{p:02}"),
                        &format!("id-{p}-{i}"),
                        "codex",
                        1,
                        &format!("Título {p}-{i}"),
                    )
                })
            })
            .collect();
        let mut page = Page::new(egui::Theme::Light, WIDE, collected(handoffs), now);
        let out = page.show();
        let mounted = rows(&out).len();
        assert!(
            (5..30).contains(&mounted),
            "{mounted} of 3.000 rows mounted"
        );
        node(&out, Role::Label, "Mostrando 3.000 de 3.000");
        let list = rect_of(&node(&out, Role::Pane, "Lista de handoffs"));
        let out = page.wheel(list.center(), -1_000_000.);
        let last = rect_of(&node(&out, Role::Button, "Título 14-199 · codex · 1 min"));
        assert!(
            list.contains_rect(last),
            "the bottom is reachable: {last:?}"
        );
        assert!(rows(&out).len() < 30, "still only what is visible");
        assert!(!has(&out, Role::Button, "Título 0-0 · codex · 1 min"));
    }

    #[test]
    fn detail_shows_meta_banner_body_and_next_steps() {
        let now = Utc::now();
        let id = "01a19ebd-3267-4c1e-9a7e-0f31b6b4677f";
        let mut cross = handoff(now, "acme / api", id, "claude-code", 12, CROSS);
        cross.next_steps = Some(vec!["Primeiro".into(), "Segundo".into()]);
        for (theme, p) in THEMES {
            let mut page = Page::new(theme, WIDE, collected(vec![cross.clone()]), now);
            let out = page.show();
            node(&out, Role::Label, "Pergunta ao dono.");
            node(&out, Role::Label, "Segundo parágrafo.\nCom linha.");
            for text in [
                "Aberto",
                "claude-code",
                &format!("{} (há 12 min)", date(&cross.at)),
                "01a19ebd-3267…6b4677f",
                "Próximos passos",
                "1. Primeiro",
                "2. Segundo",
            ] {
                node(&out, Role::Label, text);
            }
            // O nome vem decodificado e numa linha só.
            node(
                &out,
                Role::Label,
                "Mensagem entre sessões · de “Trilha \"A\" & B” — marcação técnica ocultada.",
            );
            node(&out, Role::Button, "Ver texto bruto");
            assert!(
                rects(&out)
                    .iter()
                    .any(|r| r.fill == widgets::tone(p.info, 0.10)),
                "{theme:?}: the info strip"
            );
            // A faixa vem antes do corpo, sem sobrepor (o escopo do `strip`).
            let strip = rect_of(&node(
                &out,
                Role::Label,
                "Mensagem entre sessões · de “Trilha \"A\" & B” — marcação técnica ocultada.",
            ));
            let body = rect_of(&node(&out, Role::Label, "Segundo parágrafo.\nCom linha."));
            assert!(
                body.top() - strip.bottom() >= space::S,
                "{theme:?}: {strip:?} too close to {body:?}"
            );
        }
        let plain = |summary: &str| handoff(now, "acme / api", "p", "codex", 5, summary);
        for (case, h, banner, title, body) in [
            (
                "subagent",
                plain(AGENT),
                Some("Relato de subagente — marcação técnica ocultada."),
                "Lote pronto",
                "Três casos.",
            ),
            (
                "no from-name",
                plain("<cross-session-message from=\"x\">\nOi.\n\nTchau."),
                Some("Mensagem entre sessões — marcação técnica ocultada."),
                "Oi.",
                "Tchau.",
            ),
            (
                "blank from-name",
                plain("<cross-session-message from=\"x\" from-name=\" \t\">\nOi.\n\nTchau."),
                Some("Mensagem entre sessões — marcação técnica ocultada."),
                "Oi.",
                "Tchau.",
            ),
            (
                "plain",
                plain("Resumo comum.\n\nSegue."),
                None,
                "Resumo comum.",
                "Segue.",
            ),
        ] {
            let mut page = Page::new(egui::Theme::Dark, WIDE, collected(vec![h]), now);
            let out = page.show();
            node(&out, Role::Label, title);
            node(&out, Role::Label, body);
            let strips: Vec<String> = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter_map(|(_, n)| n.value().filter(|v| v.contains("marcação técnica")))
                .map(str::to_owned)
                .collect();
            assert_eq!(strips, banner.into_iter().collect::<Vec<_>>(), "{case}");
            assert_eq!(has(&out, Role::Button, "Ver texto bruto"), banner.is_some());
            assert!(
                !has(&out, Role::Label, "Próximos passos"),
                "{case}: no next steps, no section"
            );
        }
        let mut empty_steps = plain("Resumo.");
        empty_steps.next_steps = Some(vec![]);
        let mut page = Page::new(egui::Theme::Dark, WIDE, collected(vec![empty_steps]), now);
        let out = page.show();
        node(&out, Role::Label, "Resumo.");
        assert!(!has(&out, Role::Label, "Próximos passos"));
    }

    #[test]
    fn raw_view_switches_and_copy_takes_the_text_on_screen() {
        let now = Utc::now();
        let api = collected(vec![
            handoff(now, "ws / p", "m1", "claude-code", 1, CROSS),
            handoff(now, "ws / p", "m2", "codex", 2, AGENT),
        ]);
        let mut page = Page::new(egui::Theme::Light, WIDE, api, now);
        let out = page.click_on(Role::Button, "Copiar resumo");
        assert_eq!(copied(&out), [CROSS_BODY]);
        page.click_on(Role::Button, "Ver texto bruto");
        let out = page.show();
        node(&out, Role::Label, CROSS);
        assert!(!has(&out, Role::Label, "Segundo parágrafo.\nCom linha."));
        node(&out, Role::Button, "Ver texto limpo");
        let out = page.click_on(Role::Button, "Copiar resumo");
        assert_eq!(copied(&out), [CROSS], "the raw text, untouched");
        page.click_on(Role::Button, "Ver texto limpo");
        let out = page.show();
        node(&out, Role::Label, "Segundo parágrafo.\nCom linha.");
        assert!(!has(&out, Role::Label, CROSS));
        // Bruto ligado e outra seleção: o outro abre limpo.
        page.click_on(Role::Button, "Ver texto bruto");
        page.click_on(Role::Button, "Lote pronto · codex · 2 min");
        let out = page.show();
        node(&out, Role::Label, "Três casos.");
        node(&out, Role::Button, "Ver texto bruto");
        assert!(!has(&out, Role::Label, AGENT));
    }

    #[test]
    fn copy_buttons_copy_the_exact_id_and_summary() {
        let now = Utc::now();
        let summary = "Resumo comum\r\ncom CRLF e espaço no fim  ";
        let id = "01a19ebd-3267-4c1e-9a7e-0f31b6b4677f";
        let api = collected(vec![handoff(now, "ws / p", id, "codex", 3, summary)]);
        let mut page = Page::new(egui::Theme::Dark, WIDE, api, now);
        for (role, button, want) in [
            (Role::Button, "Copiar resumo", summary),
            (Role::Button, "Copiar ID", id),
            (Role::Button, "Copiar ID do handoff", id),
        ] {
            let out = page.click_on(role, button);
            assert_eq!(copied(&out), [want], "{button}");
        }
        assert!(copied(&page.show()).is_empty(), "nothing copies by itself");
    }

    #[test]
    fn restricted_and_missing_summaries() {
        let now = Utc::now();
        let mut restricted = handoff(
            now,
            "ws / p",
            "r",
            "claude-code",
            1,
            "<cross-session-message from=\"x\" from-name=\"S\">\nsegredo-xyz",
        );
        restricted.redacted = true;
        let missing = Handoff {
            summary: None,
            ..handoff(now, "ws / p", "n", "codex", 2, "")
        };
        let rule = handoff(now, "ws / p", "h", "codex", 3, "---");
        let visible = handoff(now, "ws / p", "v", "codex", 4, "segredo-abc à vista");
        let api = collected(vec![restricted, missing, rule, visible]);
        let mut page = Page::new(egui::Theme::Dark, WIDE, api, now);
        let out = page.show();
        assert_eq!(
            rows(&out).into_iter().map(|(l, _)| l).collect::<Vec<_>>(),
            [
                "Resumo restrito · claude-code · 1 min",
                "Sem resumo · codex · 2 min",
                "Sem resumo · codex · 3 min",
                "segredo-abc à vista · codex · 4 min",
            ]
        );
        node(
            &out,
            Role::Label,
            "Resumo restrito pelas permissões do servidor.",
        );
        node(&out, Role::Button, "Copiar ID");
        assert!(!has(&out, Role::Button, "Copiar resumo"));
        assert!(!has(&out, Role::Button, "Ver texto bruto"), "no parser");
        assert!(texts(&out).iter().all(|(t, _)| !t.contains("segredo-xyz")));
        // Sem resumo: o texto fixo e nada a copiar.
        page.click_on(Role::Button, "Sem resumo · codex · 2 min");
        let out = page.show();
        node(&out, Role::Label, "Sem resumo.");
        assert!(!has(&out, Role::Button, "Copiar resumo"));
        // Sem linha útil: o título é "Sem resumo", e o texto continua lá.
        page.click_on(Role::Button, "Sem resumo · codex · 3 min");
        let out = page.show();
        node(&out, Role::Label, "---");
        let out = page.click_on(Role::Button, "Copiar resumo");
        assert_eq!(copied(&out), ["---"]);
        // A busca não alcança o resumo restrito; alcança o visível.
        page.sessions.search = "segredo-xyz".into();
        node(&page.show(), Role::Label, "Nenhum handoff aberto");
        page.sessions.search = "segredo-abc".into();
        assert_eq!(
            rows(&page.show()),
            [("segredo-abc à vista · codex · 4 min".to_owned(), true)]
        );
    }

    #[test]
    fn manage_in_review_reports_each_click_once() {
        let now = Utc::now();
        let mut page = Page::new(egui::Theme::Light, WIDE, collected(abc(now)), now);
        let out = page.show();
        assert_eq!(page.review, 0);
        let link = rect_of(&node(&out, Role::Link, "Gerenciar em Revisão →"));
        page.click(link.center());
        for _ in 0..3 {
            page.show();
        }
        assert_eq!(page.review, 1);
        page.click(link.center());
        assert_eq!(page.review, 2);
    }

    #[test]
    fn agent_filter_lists_the_agents_present_and_filters_each_tab() {
        let now = Utc::now();
        let mut api = collected(vec![
            handoff(now, "ws / p", "1", "claude-code", 1, "Um"),
            handoff(now, "ws / p", "2", "codex", 2, "Dois"),
            handoff(now, "ws / p", "3", "", 3, "Três"),
            handoff(now, "ws / p", "4", "claude-code", 4, "Quatro"),
        ]);
        api.sessions = vec![Session {
            session_id: "s1".into(),
            agent_kind: "gemini".into(),
            started_at: at(now, 5),
            scope: "ws / p".into(),
            ..Default::default()
        }];
        let mut page = Page::new(egui::Theme::Dark, WIDE, api, now);
        page.click_on(Role::ComboBox, "Agente: todos");
        let out = page.show();
        for item in ["claude-code", "codex"] {
            node(&out, Role::Button, item);
        }
        node(&out, Role::Button, "Agente: todos");
        assert!(!has(&out, Role::Button, "gemini"), "only this tab's agents");
        assert!(!has(&out, Role::Button, ""), "no blank agent");
        page.click_on(Role::Button, "codex");
        let out = page.show();
        node(&out, Role::ComboBox, "Agente: codex");
        assert_eq!(rows(&out), [("Dois · codex · 2 min".to_owned(), true)]);
        // Sessões: os agentes das sessões, mais o filtro ativo.
        page.sessions.tab = Tab::Sessions;
        let out = page.show();
        node(&out, Role::Label, "Nenhum registro");
        node(&out, Role::Label, "0 registros");
        page.click_on(Role::ComboBox, "Agente: codex");
        let out = page.show();
        node(&out, Role::Button, "gemini");
        node(&out, Role::Button, "codex");
        assert!(!has(&out, Role::Button, "claude-code"));
        page.click_on(Role::Button, "Agente: todos");
        let out = page.show();
        node(&out, Role::Label, "1 registros");
        node(&out, Role::Label, "gemini");
    }

    #[test]
    fn only_open_switch_filters_the_sessions_table() {
        let now = Utc::now();
        let session = |id: &str, ended: bool| Session {
            session_id: id.into(),
            agent_kind: "codex".into(),
            started_at: at(now, 5),
            ended_at: ended.then(|| at(now, 1)),
            scope: "ws / p".into(),
            ..Default::default()
        };
        let mut api = collected(vec![]);
        api.sessions = vec![
            session("s1", false),
            session("s2", true),
            session("s3", false),
        ];
        let mut page = Page::new(egui::Theme::Light, WIDE, api, now);
        page.sessions.tab = Tab::Sessions;
        let out = page.show();
        assert_eq!(
            node(&out, Role::CheckBox, "Só abertas").toggled(),
            Some(Toggled::True)
        );
        assert_eq!(nodes(&out, Role::Label, "Aberta").len(), 2);
        assert!(!has(&out, Role::Label, "Encerrada"));
        node(&out, Role::Label, "2 registros");
        page.click_on(Role::CheckBox, "Só abertas");
        let out = page.show();
        assert_eq!(nodes(&out, Role::Label, "Aberta").len(), 2);
        assert_eq!(nodes(&out, Role::Label, "Encerrada").len(), 1);
        node(&out, Role::Label, "3 registros");
    }

    #[test]
    fn partial_lists_warn_and_the_legend_counts_what_is_open() {
        let now = Utc::now();
        let in_project = |p: &str, n: usize| -> Vec<Handoff> {
            (0..n)
                .map(|i| handoff(now, p, &format!("{p}-{i}"), "codex", i as i64, "x"))
                .collect()
        };
        let warning = vec!["ws / a: lista limitada aos 200 handoffs mais recentes.".to_owned()];
        // (caso, api, aviso, linha de contagem, vazio)
        for (case, api, strip, count, empty) in [
            (
                "server total, two projects",
                ApiSnapshot {
                    at: Some(now),
                    handoff_total: Some(350),
                    handoffs: [in_project("ws / a", 199), in_project("ws / b", 1)].concat(),
                    warnings: warning.clone(),
                    ..Default::default()
                },
                Some("Lista: 200 handoffs carregados de 350 abertos. Até 200 por projeto."),
                Some("350 handoffs abertos em 2 projetos · visíveis para esta conexão"),
                None,
            ),
            (
                "no server total",
                ApiSnapshot {
                    at: Some(now),
                    handoffs: in_project("ws / a", 200),
                    warnings: warning.clone(),
                    ..Default::default()
                },
                Some(
                    "Lista: 200 handoffs carregados (total aberto indisponível). Até 200 por projeto.",
                ),
                Some("200+ handoffs abertos em 1 projeto · visíveis para esta conexão"),
                None,
            ),
            (
                "one",
                collected(in_project("ws / a", 1)),
                None,
                Some("1 handoff aberto em 1 projeto · visíveis para esta conexão"),
                None,
            ),
            (
                "zero",
                collected(vec![]),
                None,
                Some("0 handoffs abertos em 0 projetos · visíveis para esta conexão"),
                Some("Nenhum resultado visível para os filtros e as permissões atuais."),
            ),
            (
                "before the first collection",
                ApiSnapshot::default(),
                None,
                None,
                Some("Aguardando a consulta ao ai-memory."),
            ),
        ] {
            let mut page = Page::new(egui::Theme::Dark, WIDE, api, now);
            let out = page.show();
            node(&out, Role::Label, LIMIT);
            let labels: Vec<String> = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == Role::Label)
                .filter_map(|(_, n)| n.value().map(str::to_owned))
                .collect();
            let starting = |prefix: &str| -> Vec<&String> {
                labels.iter().filter(|l| l.starts_with(prefix)).collect()
            };
            assert_eq!(
                starting("Lista:"),
                strip.iter().collect::<Vec<_>>(),
                "{case}: strip"
            );
            assert_eq!(
                labels
                    .iter()
                    .filter(|l| l.ends_with("visíveis para esta conexão"))
                    .collect::<Vec<_>>(),
                count.iter().collect::<Vec<_>>(),
                "{case}: legend"
            );
            if let Some(subtitle) = empty {
                node(&out, Role::Label, "Nenhum handoff aberto");
                node(&out, Role::Label, subtitle);
            }
            // O aviso é da lista de handoffs; as outras abas não o mostram.
            page.sessions.tab = Tab::Sessions;
            let out = page.show();
            node(&out, Role::Label, SESSIONS_NOTE);
            assert!(
                nodes(&out, Role::Label, strip.unwrap_or("Lista:")).is_empty(),
                "{case}: no strip on Sessões"
            );
        }
    }

    #[test]
    fn long_titles_end_in_an_ellipsis_inside_the_row_and_show_whole_on_hover() {
        let now = Utc::now();
        // 97 caracteres: inteiro para o `outline`, largo demais para a linha.
        let title = "Um título muito comprido que de jeito nenhum cabe na largura da lista estreita de handoffs em 720";
        let api = collected(vec![handoff(
            now,
            "ws / p",
            "1",
            "codex",
            3,
            &format!(
                "{title}\nE uma prévia igualmente comprida que também não cabe na linha da lista estreita de jeito algum"
            ),
        )]);
        let mut page = Page::new(egui::Theme::Dark, NARROW, api, now);
        let out = page.show();
        let row = rect_of(&node(
            &out,
            Role::Button,
            &format!("{title} · codex · 3 min"),
        ));
        let cut: Vec<Rect> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.elided => Some(t.visual_bounding_rect()),
                _ => None,
            })
            .collect();
        assert_eq!(cut.len(), 2, "title and preview are both cut");
        for rect in cut {
            assert!(
                row.contains_rect(rect) && rect.right() <= row.right() - 16. + 0.5,
                "{rect:?} inside {row:?}"
            );
        }
        let tip = format!(
            "{title}\ncodex · E uma prévia igualmente comprida que também não cabe na linha da lista estreita de jeito algum"
        );
        assert!(!has(&out, Role::Label, &tip));
        // Repousa o ponteiro além do atraso do tooltip (1/60 s por frame).
        let mut out = page.frame(vec![Event::PointerMoved(row.center())]);
        for _ in 0..45 {
            out = page.show();
        }
        node(&out, Role::Label, &tip);
    }

    #[test]
    fn the_search_drops_below_the_tabs_only_when_they_leave_no_room() {
        let now = Utc::now();
        // 450 − 16 = 434: as abas (~370) deixam menos de 140 à busca.
        for (size, below) in [(NARROW, false), ([450., 600.], true)] {
            let mut api = collected(vec![Handoff::default(); 3]);
            api.handoff_total = Some(1_234_567);
            let mut page = Page::new(egui::Theme::Light, size, api, now);
            let out = page.show();
            let tabs = rect_of(&node(&out, Role::Button, "Handoffs 1.234.567"));
            let field = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, n)| n.role() == Role::TextInput && n.placeholder() == Some(SEARCH))
                .map(|(_, n)| rect_of(n))
                .expect("the search field");
            assert_eq!(field.top() > tabs.bottom(), below, "{size:?}: {field:?}");
            assert!(
                field.right() <= size[0] - 8. + 0.5 && !field.intersects(tabs),
                "{size:?}: {field:?} / {tabs:?}"
            );
        }
    }

    #[test]
    fn a_long_project_filter_is_cut_with_the_whole_name_on_hover() {
        let now = Utc::now();
        let long = "workspace-com-nome-comprido / projeto-com-um-nome-bem-longo-mesmo";
        for (scope, cut) in [(long, true), ("", false)] {
            let mut page = Page::new(egui::Theme::Dark, NARROW, collected(abc(now)), now);
            page.sessions.scope = scope.into();
            let shown = if scope.is_empty() {
                "Todos os projetos"
            } else {
                scope
            };
            let out = page.show();
            let combo = rect_of(&node(&out, Role::ComboBox, shown));
            assert!(combo.width() <= PICKER, "{shown}: {combo:?}");
            let painted = out
                .shapes
                .iter()
                .any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.elided && t.galley.text() == shown));
            assert_eq!(painted, cut, "{shown}: ellipsis");
            let mut out = page.frame(vec![Event::PointerMoved(combo.center())]);
            for _ in 0..45 {
                out = page.show();
            }
            assert_eq!(
                nodes(&out, Role::Label, shown).len(),
                usize::from(cut),
                "{shown}: tooltip"
            );
        }
    }

    #[test]
    fn tables_have_36_px_rows_their_cells_and_copy() {
        let now = Utc::now();
        let mut api = collected(vec![]);
        api.sessions = (0..500)
            .map(|i| Session {
                session_id: format!("sessao-{i:04}-0000-4000-8000-000000000000"),
                agent_kind: "codex".into(),
                started_at: at(now, i),
                observation_count: 1_234_567,
                cwd: Some("/tmp/x".into()),
                scope: "acme / api".into(),
                ..Default::default()
            })
            .collect();
        api.projects = vec![
            Project {
                workspace_name: "acme".into(),
                project_name: "api".into(),
                page_count: 1234,
                open_session_count: Some(3),
                observation_count: Some(9_876_543),
                last_activity: Some(at(now, 5)),
                ..Default::default()
            },
            Project {
                workspace_name: "acme".into(),
                project_name: "vazio".into(),
                ..Default::default()
            },
        ];
        let mut page = Page::new(egui::Theme::Dark, WIDE, api.clone(), now);
        page.sessions.tab = Tab::Sessions;
        let out = page.show();
        for column in SESSION_COLUMNS {
            node(&out, Role::Label, column.title);
        }
        let copies: Vec<Rect> = nodes(&out, Role::Button, "Copiar")
            .iter()
            .map(rect_of)
            .collect();
        assert!(
            (5..30).contains(&copies.len()),
            "{} of 500 rows mounted",
            copies.len()
        );
        let mut tops: Vec<f32> = copies.iter().map(|r| r.top()).collect();
        tops.sort_by(f32::total_cmp);
        assert!(
            tops.windows(2).all(|w| (w[1] - w[0] - 36.).abs() < 0.5),
            "rows 36 apart: {tops:?}"
        );
        assert!(nodes(&out, Role::Label, "1.234.567").len() >= 5);
        node(&out, Role::Label, "500 registros");
        let first = nodes(&out, Role::Label, "sessao-0000-0000-4")
            .first()
            .map(rect_of)
            .expect("the short ID");
        let copy = copies
            .iter()
            .find(|r| (r.center().y - first.center().y).abs() < TABLE_ROW / 2.)
            .copied()
            .unwrap();
        let out = page.click(copy.center());
        assert_eq!(copied(&out), ["sessao-0000-0000-4000-8000-000000000000"]);
        page.sessions.tab = Tab::Projects;
        let out = page.show();
        for column in PROJECT_COLUMNS {
            node(&out, Role::Label, column.title);
        }
        for text in [
            "api",
            "1.234",
            "3",
            "9.876.543",
            &date(&at(now, 5)),
            "vazio",
            "0",
        ] {
            node(&out, Role::Label, text);
        }
        assert_eq!(
            nodes(&out, Role::Label, "—").len(),
            3,
            "unknown open, events and activity are —, not 0"
        );
    }

    #[test]
    fn search_matches_project_agent_id_and_summary_and_is_typed_in_the_field() {
        let now = Utc::now();
        let api = collected(vec![
            handoff(
                now,
                "ws / alfa",
                "id-1",
                "claude-code",
                1,
                "Primeiro resumo",
            ),
            handoff(now, "ws / beta", "id-2", "codex", 2, "Parágrafo único"),
        ]);
        let mut page = Page::new(egui::Theme::Light, WIDE, api, now);
        for (needle, want) in [
            ("", vec!["Primeiro resumo", "Parágrafo único"]),
            ("beta", vec!["Parágrafo único"]),
            ("CODEX", vec!["Parágrafo único"]),
            ("id-1", vec!["Primeiro resumo"]),
            ("parágrafo", vec!["Parágrafo único"]),
            ("ws /", vec!["Primeiro resumo", "Parágrafo único"]),
            ("gama", vec![]),
        ] {
            page.sessions.search = needle.into();
            let titles: Vec<String> = rows(&page.show())
                .into_iter()
                .map(|(l, _)| l.split(" · ").next().unwrap().to_owned())
                .collect();
            assert_eq!(titles, want, "{needle:?}");
        }
        page.sessions.search.clear();
        let out = page.show();
        let field = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, n)| n.role() == Role::TextInput && n.placeholder() == Some(SEARCH))
            .and_then(|(_, n)| n.bounds())
            .expect("the search field");
        page.click(pos2(
            ((field.x0 + field.x1) / 2.) as f32,
            ((field.y0 + field.y1) / 2.) as f32,
        ));
        page.frame(vec![Event::Text("beta".into())]);
        assert_eq!(page.sessions.search, "beta");
        assert_eq!(
            rows(&page.show()),
            [("Parágrafo único · codex · 2 min".to_owned(), true)]
        );
    }

    #[test]
    fn reset_clears_data_filters_and_selection_but_keeps_tab_search_and_switch() {
        let mut page = Sessions {
            tab: Tab::Sessions,
            search: "busca".into(),
            scope: "ws / p".into(),
            agent: "codex".into(),
            only_open: false,
            selected: Some(("ws / p".into(), "id".into())),
            open: true,
            raw: true,
            ..Default::default()
        };
        page.reset();
        assert_eq!(
            (
                page.tab,
                page.search.as_str(),
                page.scope.as_str(),
                page.agent.as_str(),
                page.only_open,
                page.selected.clone(),
                page.open,
                page.raw
            ),
            (Tab::Sessions, "busca", "", "", false, None, false, false)
        );
    }

    #[test]
    fn short_ids_keep_head_and_tail_and_banner_names_are_one_line_text() {
        for (id, want) in [
            (
                "01a19ebd-3267-4c1e-9a7e-0f31b6b4677f",
                "01a19ebd-3267…6b4677f",
            ),
            ("1234567890123456789012", "1234567890123456789012"),
            ("12345678901234567890123", "1234567890123…7890123"),
            ("", ""),
            ("ação-ação-ação-ação-ação-ç", "ação-ação-açã…-ação-ç"),
        ] {
            assert_eq!(middle(id), want, "{id}");
        }
        let name = |n: &str| {
            banner(&Origin::CrossSession {
                from: "x".into(),
                name: Some(n.into()),
            })
            .1
        };
        assert_eq!(
            name("Trilha\n\tA\u{7}  <b>B</b>"),
            "· de “Trilha A <b>B</b>” — marcação técnica ocultada."
        );
        assert_eq!(name(" \n "), "— marcação técnica ocultada.");
        assert_eq!(
            banner(&Origin::Subagent { from: "a".into() }),
            ("Relato de subagente", "— marcação técnica ocultada.".into())
        );
    }
}
