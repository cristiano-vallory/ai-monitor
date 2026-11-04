//! Página Visão geral (Main.dc.html): faixa de saúde, quatro indicadores, o
//! cartão Recursos com a área empilhada e, embaixo, os contêineres em execução
//! e os handoffs recentes. Fica na rolagem da área principal e cabe na
//! largura: tudo sai da largura do frame, nada do layout anterior é guardado.
//! Só consulta.
//!
//! Regras desta tela:
//! - Sem dado = "—", nunca 0. Sparkline e gráfico sem amostras não desenham
//!   linha nenhuma.
//! - Indicadores (mín. 220): os quatro numa linha quando cabem, senão 2 + 2
//!   e, abaixo de dois, um por linha (`kpi_lines`). A linha de baixo
//!   (`3 1 420px` e `2 1 300px`) quebra como o `flex-wrap` do artboard
//!   (`flex_lines`). Gap 12; na mesma linha, a mesma altura.
//! - Outros contêineres = total do Docker − ai-memory, nunca negativo, só
//!   quando o ai-memory medido é o contêiner monitorado em execução; com o
//!   ai-memory nativo (fora do Docker) ou sem medida, o total do Docker. O
//!   cartão e o gráfico fazem a mesma conta (`stack`).
//! - Gráfico, sparklines, média e pico na janela escolhida (1, 5 ou 10 min até
//!   a última amostra), com as lacunas da coleta: amostra isolada = ponto;
//!   intervalo acima de 3 coletas (mín. 65 s), tempo que volta ou amostra sem
//!   o valor = a linha quebra. O eixo x do gráfico é a janela inteira; o da
//!   sparkline (sem eixo) vai da 1ª amostra da janela à última.
//! - Continuidade: handoffs = total do servidor ("N+" sem o total, "—" antes da
//!   1ª coleta); sessões abertas = `open_session_links` (o mesmo número da aba
//!   Sessões abertas); propostas só com projeto vigiado (`watched_pending`;
//!   desconhecido = "—").
//! - Handoffs recentes: os 4 primeiros da coleta (do mais novo ao mais velho),
//!   com o título de `sessions_ui::headline` (restrito e sem resumo inclusos).
use crate::{
    docker_ui,
    health::{self, Snapshot},
    health_ui::{self, Overall},
    model::{
        ApiSnapshot, Config, ResourceSample, ResourceSnapshot, Usage, bytes, decimal,
        decimal_up_to, number,
    },
    sessions_ui, theme,
    widgets::{self, Segment, Type},
};
use chrono::{DateTime, Local, Utc};
use eframe::egui::{
    self, Align, Color32, FontId, Galley, Layout, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
    UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::Value;
use std::{collections::VecDeque, sync::Arc};

type Sample = ResourceSample;

const GAP: f32 = 12.;
const KPI_MIN: f32 = 220.;
const SPARK_WIDTH: f32 = 84.;
const SPARK_HEIGHT: f32 = 22.;
const PLOT_HEIGHT: f32 = 178.;
/// Coluna dos rótulos do eixo y e o espaço até o gráfico.
const AXIS: f32 = 34.;
const AXIS_GAP: f32 = 10.;
const HEAD_ROW: f32 = 28.;
const ROW: f32 = 36.;
/// Colunas fixas da tabela de contêineres, com o respiro (Nome fica com o
/// resto): CPU e Memória 8 + 8, Tarefas 8 + 16.
const CPU_COLUMN: f32 = 80.;
const MEMORY_COLUMN: f32 = 104.;
const TASKS_COLUMN: f32 = 88.;
const RECENT: usize = 4;
const MIB: f64 = (1u64 << 20) as f64;
const GIB: f64 = (1u64 << 30) as f64;
const OTHERS_HINT: &str =
    "Soma dos outros contêineres em execução: o total do Docker menos o ai-memory.";
const CONTINUITY_HINT: &str = "Handoffs visíveis para esta conexão. Sessões abertas: vínculos por projeto informados pela API; podem se repetir entre projetos.";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Metric {
    Cpu,
    Memory,
}
impl Metric {
    fn of(self, u: &Usage) -> f64 {
        match self {
            Metric::Cpu => u.cpu,
            Metric::Memory => u.memory as f64,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Metric::Cpu => "CPU",
            Metric::Memory => "Memória",
        }
    }
    fn amount(self, v: Option<f64>) -> String {
        match v {
            None => "—".into(),
            Some(v) => match self {
                Metric::Cpu => format!("{}%", decimal(v, 1)),
                Metric::Memory => bytes(v as u64),
            },
        }
    }
}

/// Métrica e janela do cartão Recursos; ficam ao trocar de página e de
/// conexão.
pub(crate) struct Overview {
    metric: Metric,
    minutes: i64,
}
impl Default for Overview {
    fn default() -> Self {
        Self {
            metric: Metric::Cpu,
            minutes: 5,
        }
    }
}

/// Para onde um link ou um clique da página leva.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Go {
    Health,
    Sessions,
    /// O Docker, com este contêiner no inspetor (`None` = "Ver todos →").
    Docker(Option<String>),
}

pub(crate) struct Data<'a> {
    pub resources: &'a ResourceSnapshot,
    pub history: &'a VecDeque<Sample>,
    pub api: &'a ApiSnapshot,
    pub health: &'a Snapshot,
    pub config: &'a Config,
    pub now: DateTime<Utc>,
}

impl Overview {
    pub(crate) fn show(&mut self, ui: &mut Ui, d: &Data<'_>) -> Option<Go> {
        let mut go = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 14.;
            if health_strip(ui, d.health, d.now) {
                go = Some(Go::Health);
            }
            kpis(ui, d, self.minutes);
            self.resources(ui, d);
            if let Some(next) = bottom(ui, d) {
                go = Some(next);
            }
        });
        go
    }

    /// Título, legenda e os dois segmentados numa linha (ou os segmentados
    /// embaixo, se não couberem), o gráfico e o eixo do tempo.
    fn resources(&mut self, ui: &mut Ui, d: &Data<'_>) {
        let p = theme::palette(ui.visuals());
        let metrics = [Metric::Cpu, Metric::Memory].map(|value| Segment {
            value,
            label: value.label(),
            count: None,
        });
        let windows = [(1, "1 min"), (5, "5 min"), (10, "10 min")].map(|(value, label)| Segment {
            value,
            label,
            count: None,
        });
        let response = widgets::card(ui)
            .inner_margin(egui::Margin {
                left: 16,
                right: 16,
                top: 14,
                bottom: 12,
            })
            .show(ui, |ui| {
                let width = ui.available_width();
                ui.set_min_width(width);
                ui.spacing_mut().item_spacing = vec2(16., 10.);
                let title =
                    ui.painter()
                        .layout_no_wrap("Recursos".into(), Type::Section.font(), p.text);
                let legend = [(p.acc, "ai-memory"), (p.series2, "Outros contêineres")];
                let legend_width = legend_width(ui, &legend);
                let controls = widgets::segmented_width(ui, &self.metric, &metrics)
                    + 16.
                    + widgets::segmented_width(ui, &self.minutes, &windows);
                let one_row = title.size().x + 16. + legend_width + 16. + controls <= width;
                // Os segmentados vão da esquerda para a direita mesmo
                // encostados à direita: num leiaute da direita para a
                // esquerda, a ordem entre os dois se inverteria.
                ui.horizontal(|ui| {
                    ui.set_min_height(30.);
                    ui.label(egui::RichText::new("Recursos").font(Type::Section.font()));
                    legend_items(ui, &legend);
                    if one_row {
                        ui.add_space((ui.available_width() - controls).max(0.));
                        widgets::segmented(ui, &mut self.metric, &metrics);
                        widgets::segmented(ui, &mut self.minutes, &windows);
                    }
                });
                if !one_row {
                    ui.horizontal(|ui| {
                        ui.set_min_height(30.);
                        widgets::segmented(ui, &mut self.metric, &metrics);
                        widgets::segmented(ui, &mut self.minutes, &windows);
                    });
                }
                chart(
                    ui,
                    d.history,
                    self.minutes,
                    self.metric,
                    d.config.resource_seconds,
                );
            })
            .response;
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Panel, true, "Recursos"));
    }
}

/// `flex-wrap` de itens `flex: grow 1 base`, com `gap`: cada linha leva os
/// itens que cabem pela base (ao menos um); a sobra vai aos itens da linha na
/// proporção do `grow` (e a falta sai na proporção da base). Devolve, por
/// linha, (índice, largura).
pub(crate) fn flex_lines(width: f32, items: &[(f32, f32)], gap: f32) -> Vec<Vec<(usize, f32)>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.;
    for (i, &(basis, _)) in items.iter().enumerate() {
        match lines.last_mut() {
            Some(line) if used + gap + basis <= width => {
                line.push(i);
                used += gap + basis;
            }
            _ => {
                lines.push(vec![i]);
                used = basis;
            }
        }
    }
    lines
        .into_iter()
        .map(|line| {
            let bases: f32 = line.iter().map(|&i| items[i].0).sum();
            let grows: f32 = line.iter().map(|&i| items[i].1).sum();
            let free = width - bases - gap * (line.len() - 1) as f32;
            line.iter()
                .map(|&i| {
                    let (basis, grow) = items[i];
                    let extra = if free >= 0. {
                        free * grow / grows
                    } else {
                        free * basis / bases
                    };
                    (i, basis + extra)
                })
                .collect()
        })
        .collect()
}

/// Os quatro indicadores numa linha quando cabem (`KPI_MIN` cada, com o
/// gap); senão dois por linha (2 + 2, nunca 3 + 1), e um por linha abaixo de
/// dois. Na linha, larguras iguais. Devolve, por linha, (índice, largura).
pub(crate) fn kpi_lines(width: f32) -> Vec<Vec<(usize, f32)>> {
    let fits = |n: f32| n * KPI_MIN + (n - 1.) * GAP <= width;
    let per_line = if fits(4.) {
        4
    } else if fits(2.) {
        2
    } else {
        1
    };
    let each = (width - GAP * (per_line - 1) as f32) / per_line as f32;
    (0..4)
        .collect::<Vec<usize>>()
        .chunks(per_line)
        .map(|line| line.iter().map(|&i| (i, each)).collect())
        .collect()
}

/// Cartões nas `lines` (índice, largura), com a mesma altura na linha
/// (`widgets::card_row`); cada um é um nó `Pane` com o seu título.
fn cards(
    ui: &mut Ui,
    titles: &[&str],
    lines: Vec<Vec<(usize, f32)>>,
    margin: egui::Margin,
    mut add: impl FnMut(&mut Ui, usize),
) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = GAP;
        for line in lines {
            widgets::card_row(
                ui,
                titles,
                &line,
                margin,
                |index| ("overview-card", titles[index]),
                &mut add,
            );
        }
    });
}

/// Texto pintado com um nó AccessKit `Label` (e o tooltip, se houver).
fn spoken(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash, text: &str, tip: Option<String>) {
    let response = ui.interact(rect, ui.id().with(id), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    if let Some(tip) = tip {
        response.on_hover_text(tip);
    }
}
/// Link de 12 px (acc) encostado à direita em `right`, centrado em `y`;
/// `true` no clique.
fn link(ui: &mut Ui, right: f32, y: f32, text: &str) -> bool {
    let p = theme::palette(ui.visuals());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(12.), p.acc);
    let at = Rect::from_min_max(
        pos2(right - galley.size().x - 4., y - galley.size().y / 2. - 2.),
        pos2(right, y + galley.size().y / 2. + 2.),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(at)
            .layout(Layout::right_to_left(Align::Center)),
    );
    // Com seleção de texto, o egui anuncia o link como Label.
    child.style_mut().interaction.selectable_labels = false;
    child
        .add(egui::Link::new(
            egui::RichText::new(text).size(12.).color(p.acc),
        ))
        .clicked()
}

// ---- Faixa de saúde ----

/// Título e resumo da faixa para cada estado de `health_ui::overall`.
fn strip_text(s: &Snapshot, state: &Overall) -> (&'static str, String) {
    match state {
        Overall::Healthy => {
            let facts = facts(s);
            (
                "Tudo funcionando",
                if facts.is_empty() {
                    "Sem alertas nas verificações disponíveis.".into()
                } else {
                    facts.join(" · ")
                },
            )
        }
        Overall::Alerts(_) => (state.title(), s.alerts().join(" · ")),
        Overall::Stale => (
            state.title(),
            format!("A última coleta tem mais de {} s.", health::INTERVAL * 3),
        ),
        Overall::Waiting | Overall::Unavailable(_) => (state.title(), state.subtitle()),
    }
}
/// O que a leitura saudável sabe: versão, providers, consolidação e fila de
/// escrita. O que falta fica de fora (desconhecido não vira 0 nem "ok").
fn facts(s: &Snapshot) -> Vec<String> {
    let v = s.status.as_ref().unwrap_or(&Value::Null);
    let mut out = Vec::new();
    let version = v["version"]
        .as_str()
        .or(s.identity.as_ref().map(|i| i.version.as_str()))
        .filter(|v| !v.is_empty());
    if let Some(version) = version {
        out.push(format!("ai-memory {version}"));
    }
    let ok = |role: &str| {
        matches!(
            v["providers"][role]["status"].as_str(),
            Some("ok" | "healthy")
        )
    };
    if ok("llm") && ok("embedding") {
        out.push("providers ok".into());
    }
    match v["consolidation"]["pending"].as_u64() {
        Some(0) => out.push("consolidação sem pendências".into()),
        Some(1) => out.push("consolidação: 1 pendente".into()),
        Some(n) => out.push(format!("consolidação: {} pendentes", number(n as usize))),
        None => {}
    }
    if let (Some(queued), Some(capacity)) =
        (v["write_queue"][0].as_u64(), v["write_queue"][1].as_u64())
    {
        out.push(format!(
            "fila de escrita {} de {}",
            number(queued as usize),
            number(capacity as usize)
        ));
    }
    out
}
/// Saudável: círculo com o visto (o path do artboard); os outros, os ícones
/// da página Saúde.
fn strip_icon(painter: &egui::Painter, rect: Rect, state: &Overall, color: Color32) {
    if *state == Overall::Healthy {
        painter.circle_stroke(
            rect.center(),
            9. * rect.width() / 24.,
            Stroke::new(2. * rect.width() / 24., color),
        );
        widgets::strokes(
            painter,
            rect,
            &[&[(8., 12.5), (10.6, 15.), (16., 9.5)]],
            2.,
            color,
        );
    } else {
        health_ui::state_icon(painter, rect, state, color);
    }
}
/// Ícone, título, resumo (ao lado se sobrarem 320; senão embaixo) e "Ver
/// saúde →". `true` no link.
fn health_strip(ui: &mut Ui, s: &Snapshot, now: DateTime<Utc>) -> bool {
    let p = theme::palette(ui.visuals());
    let state = health_ui::overall(s, now);
    let (title, summary) = strip_text(s, &state);
    let color = state.tone(p).1;
    let mut clicked = false;
    let response = widgets::card(ui)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            let title_galley = ui.painter().layout_no_wrap(
                title.into(),
                FontId::new(13., theme::semibold()),
                p.text,
            );
            let link_width = ui
                .painter()
                .layout_no_wrap("Ver saúde →".into(), FontId::proportional(12.), p.acc)
                .size()
                .x;
            let fixed = 18. + 12. + title_galley.size().x + 12. + 12. + link_width;
            let beside = width - fixed >= 320.;
            let summary_width = if beside { width - fixed } else { width };
            let summary_galley = ui.painter().layout(
                summary.clone(),
                FontId::proportional(13.),
                p.text2,
                summary_width,
            );
            let head = title_galley.size().y.max(18.);
            let height = if beside {
                head.max(summary_galley.size().y)
            } else {
                head + 6. + summary_galley.size().y
            };
            let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let y = if beside {
                rect.center().y
            } else {
                rect.top() + head / 2.
            };
            let icon = Rect::from_min_size(pos2(rect.left(), y - 9.), vec2(18., 18.));
            strip_icon(ui.painter(), icon, &state, color);
            let title_at = pos2(icon.right() + 12., y - title_galley.size().y / 2.);
            let title_rect = Rect::from_min_size(title_at, title_galley.size());
            ui.painter().galley(title_at, title_galley, p.text);
            spoken(ui, title_rect, "strip-title", title, None);
            let summary_at = if beside {
                pos2(
                    title_rect.right() + 12.,
                    rect.center().y - summary_galley.size().y / 2.,
                )
            } else {
                pos2(rect.left(), rect.top() + head + 6.)
            };
            let summary_rect = Rect::from_min_size(summary_at, summary_galley.size());
            ui.painter().galley(summary_at, summary_galley, p.text2);
            spoken(ui, summary_rect, "strip-summary", &summary, None);
            clicked = link(ui, rect.right(), y, "Ver saúde →");
        })
        .response;
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Panel, true, "Saúde"));
    clicked
}

// ---- Indicadores ----

/// As amostras da janela: de `minutes` antes da última até ela.
fn window(history: &VecDeque<Sample>, minutes: i64) -> Vec<&Sample> {
    let Some(end) = history.back().map(|s| s.at) else {
        return Vec::new();
    };
    let start = end - chrono::Duration::minutes(minutes);
    history.iter().filter(|s| s.at >= start).collect()
}
/// "421,0 MiB" em valor e unidade.
fn split_unit(text: String) -> (String, String) {
    match text.rsplit_once(' ') {
        Some((value, unit)) => (value.to_owned(), unit.to_owned()),
        None => (text, String::new()),
    }
}

struct Kpi {
    title: &'static str,
    /// Valor e unidade; `None` = "—" (sem unidade).
    value: Option<(String, String)>,
    sub: String,
    hint: Option<&'static str>,
    /// A série da sparkline (ai-memory), se o cartão tem uma.
    spark: Option<Metric>,
}
fn kpi_list(d: &Data<'_>, minutes: i64) -> [Kpi; 4] {
    let r = d.resources;
    // Uma medida só do ai-memory por frame: o valor e a amostra (`others`).
    let measure = r.ai_measure(&d.config.container);
    let ai = measure.as_ref().map(|(usage, _)| usage.clone());
    let samples = window(d.history, minutes);
    let ai_values = |m: Metric| -> Vec<f64> {
        samples
            .iter()
            .filter_map(|s| s.ai.as_ref().map(|u| m.of(u)))
            .collect()
    };
    let cpu = ai_values(Metric::Cpu);
    let average = (!cpu.is_empty()).then(|| cpu.iter().sum::<f64>() / cpu.len() as f64);
    let peak = ai_values(Metric::Memory)
        .into_iter()
        .reduce(f64::max)
        .map(|v| bytes(v as u64));
    let docker = r.at.is_some() && r.docker_error.is_none();
    // A mesma conta do gráfico (`stack`), na coleta atual.
    let others = r.sample_of(measure).and_then(|s| {
        let cpu = stack(&s, Metric::Cpu).1?;
        let memory = stack(&s, Metric::Memory).1?;
        Some(format!("{}% · {}", decimal(cpu, 1), bytes(memory as u64)))
    });
    let handoffs = d.api.open_handoffs().map(|(_, text)| text);
    let sessions = match d.api.open_session_links() {
        Some(1) => "1 sessão aberta".to_owned(),
        Some(n) => format!("{} sessões abertas", number(n as usize)),
        None => "— sessões abertas".to_owned(),
    };
    let proposals = d.config.proposal_watch.as_ref().map(|watch| {
        match d.health.watched_pending(watch, d.now) {
            Some(1) => " · 1 proposta".to_owned(),
            Some(n) => format!(" · {} propostas", health::pending_text(n)),
            None => " · — propostas".to_owned(),
        }
    });
    [
        Kpi {
            title: "ai-memory · CPU",
            value: ai.as_ref().map(|u| (decimal(u.cpu, 1), "%".to_owned())),
            sub: format!(
                "100% = 1 núcleo · média {minutes} min {}",
                Metric::Cpu.amount(average)
            ),
            hint: None,
            spark: Some(Metric::Cpu),
        },
        Kpi {
            title: "ai-memory · Memória",
            value: ai.as_ref().map(|u| split_unit(bytes(u.memory))),
            sub: format!("pico em {minutes} min: {}", peak.as_deref().unwrap_or("—")),
            hint: None,
            spark: Some(Metric::Memory),
        },
        Kpi {
            title: "Contêineres",
            value: docker.then(|| {
                (
                    number(r.containers.iter().filter(|c| c.state == "running").count()),
                    format!("de {} ativos", number(r.containers.len())),
                )
            }),
            sub: format!("demais contêineres: {}", others.as_deref().unwrap_or("—")),
            hint: Some(OTHERS_HINT),
            spark: None,
        },
        Kpi {
            title: "Continuidade",
            value: handoffs.map(|n| {
                let unit = if n == "1" {
                    "handoff aberto"
                } else {
                    "handoffs abertos"
                };
                (n, unit.to_owned())
            }),
            sub: format!("{sessions}{}", proposals.unwrap_or_default()),
            hint: Some(CONTINUITY_HINT),
            spark: None,
        },
    ]
}
fn kpis(ui: &mut Ui, d: &Data<'_>, minutes: i64) {
    let list = kpi_list(d, minutes);
    let titles = list.each_ref().map(|k| k.title);
    let lines = kpi_lines(ui.available_width());
    cards(
        ui,
        &titles,
        lines,
        egui::Margin {
            left: 16,
            right: 16,
            top: 14,
            bottom: 12,
        },
        |ui, i| kpi(ui, &list[i], d, minutes),
    );
}
/// Título (12, text2) e sparkline 84 × 22 à direita; valor (24/600) e
/// unidade (13, text2) numa linha só; a linha de baixo (12, text2).
fn kpi(ui: &mut Ui, k: &Kpi, d: &Data<'_>, minutes: i64) {
    let p = theme::palette(ui.visuals());
    ui.spacing_mut().item_spacing.y = 2.;
    let width = ui.available_width();
    let (head, _) = ui.allocate_exact_size(vec2(width, SPARK_HEIGHT), Sense::hover());
    let room = if k.spark.is_some() {
        width - SPARK_WIDTH - 8.
    } else {
        width
    };
    let title = widgets::one_line(ui, k.title, Type::Caption.font(), p.text2, room);
    let title_rect = Rect::from_min_size(
        pos2(head.left(), head.center().y - title.size().y / 2.),
        title.size(),
    );
    let cut = title.elided.then(|| k.title.to_owned());
    ui.painter().galley(title_rect.min, title, p.text2);
    spoken(ui, title_rect, (k.title, "title"), k.title, cut);
    if let Some(metric) = k.spark {
        let rect = Rect::from_min_size(
            pos2(head.right() - SPARK_WIDTH, head.top()),
            vec2(SPARK_WIDTH, SPARK_HEIGHT),
        );
        sparkline(
            ui.painter(),
            rect,
            d.history,
            minutes,
            d.config.resource_seconds,
            metric,
            p.acc,
        );
    }
    let (value_text, value) = value_line(ui, k, width);
    let (rect, _) = ui.allocate_exact_size(vec2(width, value.size().y), Sense::hover());
    let cut = value.elided.then(|| value_text.clone());
    ui.painter().galley(rect.min, value, p.text);
    spoken(ui, rect, (k.title, "value"), &value_text, cut);
    let sub = widgets::one_line(ui, &k.sub, Type::Caption.font(), p.text2, width);
    let (rect, _) = ui.allocate_exact_size(vec2(width, sub.size().y), Sense::hover());
    let tip = match (sub.elided, k.hint) {
        (true, Some(hint)) => Some(format!("{}\n\n{hint}", k.sub)),
        (true, None) => Some(k.sub.clone()),
        (false, hint) => hint.map(str::to_owned),
    };
    ui.painter().galley(rect.min, sub, p.text2);
    spoken(ui, rect, (k.title, "sub"), &k.sub, tip);
}
/// O valor e a unidade num galley de uma linha (corta, nunca quebra), e o
/// texto falado ("12,4 %", "—").
fn value_line(ui: &Ui, k: &Kpi, width: f32) -> (String, Arc<Galley>) {
    let p = theme::palette(ui.visuals());
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping::truncate_at_width(width),
        ..Default::default()
    };
    let text = match &k.value {
        Some((value, unit)) => {
            job.append(
                value,
                0.,
                egui::TextFormat::simple(Type::Metric.font(), p.text),
            );
            job.append(
                unit,
                6.,
                egui::TextFormat::simple(Type::Body.font(), p.text2),
            );
            format!("{value} {unit}")
        }
        None => {
            job.append(
                "—",
                0.,
                egui::TextFormat::simple(Type::Metric.font(), p.text),
            );
            "—".into()
        }
    };
    (text, ui.painter().layout_job(job))
}
/// A série do ai-memory na janela, de 0 ao pico, da 1ª amostra da janela à
/// última (`Axis::Samples`); sem amostras, nada.
fn sparkline(
    painter: &egui::Painter,
    rect: Rect,
    history: &VecDeque<Sample>,
    minutes: i64,
    resource_seconds: u64,
    metric: Metric,
    color: Color32,
) {
    let Some(peak) = window(history, minutes)
        .iter()
        .filter_map(|s| s.ai.as_ref().map(|u| metric.of(u)))
        .reduce(f64::max)
    else {
        return;
    };
    let max = if peak > 0. { peak } else { 1. };
    let plot = rect.shrink2(vec2(1., 2.));
    let (lines, dots) = segments(history, minutes, resource_seconds, max, plot, |s| {
        s.ai.as_ref().map(|u| metric.of(u))
    });
    for line in lines {
        painter.add(Shape::line(line, Stroke::new(1.5_f32, color)));
    }
    for dot in dots {
        painter.circle_filled(dot, 1.5, color);
    }
}

// ---- Gráfico ----

/// O eixo x de uma série.
#[derive(Clone, Copy)]
enum Axis {
    /// A janela inteira, de `minutes` antes da última amostra até ela (o
    /// gráfico, que mostra o eixo).
    Window,
    /// Da 1ª amostra dentro da janela até a última (a sparkline, sem eixo):
    /// poucos segundos de histórico ocupam a largura toda; uma amostra só
    /// fica na borda direita.
    Samples,
}
/// Trechos contínuos da janela: (posição no `axis`, de 0 a 1, valor). Quebra
/// quando o intervalo passa de 3 coletas (mín. 65 s), quando o tempo volta ou
/// quando a amostra não tem o valor (que conta como a anterior no intervalo).
fn runs<T>(
    history: &VecDeque<Sample>,
    minutes: i64,
    resource_seconds: u64,
    axis: Axis,
    value: impl Fn(&Sample) -> Option<T>,
) -> Vec<Vec<(f32, T)>> {
    let end = history.back().map(|s| s.at).unwrap_or_else(Utc::now);
    let start = end - chrono::Duration::minutes(minutes);
    let origin = match axis {
        Axis::Window => start,
        Axis::Samples => history.iter().find(|s| s.at >= start).map_or(end, |s| s.at),
    };
    let span = (end - origin).num_milliseconds() as f32;
    let gap_limit = (resource_seconds.saturating_mul(3).max(65)) as i64;
    let mut runs = Vec::new();
    let mut run = Vec::new();
    let mut previous: Option<DateTime<Utc>> = None;
    for s in history.iter().filter(|s| s.at >= start) {
        let v = value(s);
        if (previous.is_some_and(|at| (s.at - at).num_seconds() > gap_limit || s.at < at)
            || v.is_none())
            && !run.is_empty()
        {
            runs.push(std::mem::take(&mut run));
        }
        if let Some(v) = v {
            let x = if span > 0. {
                ((s.at - origin).num_milliseconds() as f32 / span).clamp(0., 1.)
            } else {
                1.
            };
            run.push((x, v));
        }
        previous = Some(s.at);
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}
fn plot_x(plot: Rect, x: f32) -> f32 {
    plot.left() + plot.width() * x
}
fn plot_y(plot: Rect, v: f64, max: f64) -> f32 {
    (plot.bottom() - plot.height() * (v / max) as f32).clamp(plot.top(), plot.bottom())
}
/// Linhas (trechos de 2+ amostras) e pontos (amostra isolada) de uma série
/// da sparkline (`Axis::Samples`).
fn segments(
    history: &VecDeque<Sample>,
    minutes: i64,
    resource_seconds: u64,
    max: f64,
    plot: Rect,
    value: impl Fn(&Sample) -> Option<f64>,
) -> (Vec<Vec<Pos2>>, Vec<Pos2>) {
    let mut lines = Vec::new();
    let mut dots = Vec::new();
    for run in runs(history, minutes, resource_seconds, Axis::Samples, value) {
        let points: Vec<Pos2> = run
            .into_iter()
            .map(|(x, v)| pos2(plot_x(plot, x), plot_y(plot, v, max)))
            .collect();
        if points.len() > 1 {
            lines.push(points);
        } else {
            dots.extend(points);
        }
    }
    (lines, dots)
}
/// A faixa entre `top` e `base` de um trecho, em quadriláteros.
fn band(mesh: &mut egui::Mesh, points: &[(f32, f32, f32)], color: Color32) {
    for pair in points.windows(2) {
        let [(x0, top0, base0), (x1, top1, base1)] = [pair[0], pair[1]];
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(x0, top0), color);
        mesh.colored_vertex(pos2(x1, top1), color);
        mesh.colored_vertex(pos2(x1, base1), color);
        mesh.colored_vertex(pos2(x0, base0), color);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
}
/// O topo do eixo y: CPU no próximo 1, 2, 2,5 ou 5 × 10ⁿ a partir de 115% do
/// pico (mín. 10%); memória na próxima potência de 2 (mín. 64 MiB).
fn scale(metric: Metric, peak: f64) -> f64 {
    match metric {
        Metric::Cpu => {
            let x = (peak * 1.15).max(10.);
            let exp = 10f64.powf(x.log10().floor());
            let f = x / exp;
            [1., 2., 2.5, 5., 10.]
                .into_iter()
                .find(|&step| f <= step + 1e-9)
                .unwrap_or(10.)
                * exp
        }
        Metric::Memory => {
            let x = (peak * 1.15).max(64. * MIB);
            let mut max = 64. * MIB;
            while max < x {
                max *= 2.;
            }
            max
        }
    }
}
/// Rótulo do eixo y na unidade do topo.
fn axis_label(metric: Metric, v: f64, max: f64) -> String {
    match metric {
        Metric::Cpu => format!("{}%", decimal_up_to(v, 1)),
        Metric::Memory if max >= GIB => format!("{} GiB", decimal_up_to(v / GIB, 1)),
        Metric::Memory => format!("{} MiB", decimal_up_to(v / MIB, 0)),
    }
}
/// ai-memory, outros e o total do Docker de uma amostra. Outros = o total
/// menos o ai-memory (≥ 0) só quando ele é o contêiner monitorado; nativo ou
/// sem medida, ele não está no total e os outros são o total.
fn stack(s: &Sample, metric: Metric) -> (Option<f64>, Option<f64>, Option<f64>) {
    let ai = s.ai.as_ref().map(|u| metric.of(u));
    let total = s.docker.as_ref().map(|u| metric.of(u));
    let inside = if s.ai_in_docker { ai.unwrap_or(0.) } else { 0. };
    let others = total.map(|t| (t - inside).max(0.));
    (ai, others, total)
}
fn chart(
    ui: &mut Ui,
    history: &VecDeque<Sample>,
    minutes: i64,
    metric: Metric,
    resource_seconds: u64,
) {
    let p = theme::palette(ui.visuals());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, PLOT_HEIGHT), Sense::hover());
    let plot = Rect::from_min_max(pos2(rect.left() + AXIS + AXIS_GAP, rect.top()), rect.max);
    let samples = window(history, minutes);
    // O topo da pilha: o ai-memory mais os outros.
    let peak = samples
        .iter()
        .map(|s| {
            let (ai, others, _) = stack(s, metric);
            ai.unwrap_or(0.) + others.unwrap_or(0.)
        })
        .fold(0., f64::max);
    let max = scale(metric, peak);
    let painter = ui.painter();
    for (i, y) in [plot.top() + 0.5, plot.center().y, plot.bottom() - 0.5]
        .into_iter()
        .enumerate()
    {
        painter.hline(plot.x_range(), y, Stroke::new(1.0_f32, p.line));
        painter.text(
            pos2(plot.left() - AXIS_GAP, y),
            egui::Align2::RIGHT_CENTER,
            axis_label(metric, max * (1. - i as f64 / 2.), max),
            FontId::proportional(11.),
            p.text2,
        );
    }
    let clip = painter.with_clip_rect(plot.expand(2.));
    let at = |x: f32, v: f64| pos2(plot_x(plot, x), plot_y(plot, v, max));
    // Outros, empilhados sobre o ai-memory, e a linha do total.
    let stacked = runs(history, minutes, resource_seconds, Axis::Window, |s| {
        let (ai, others, _) = stack(s, metric);
        let base = ai.unwrap_or(0.);
        others.map(|o| (base, base + o))
    });
    let ai = runs(history, minutes, resource_seconds, Axis::Window, |s| {
        stack(s, metric).0
    });
    for (series, color, alpha, width) in
        [(true, p.series2, 0.22, 1.5_f32), (false, p.acc, 0.28, 1.75)]
    {
        let bands: Vec<Vec<(f32, f64, f64)>> = if series {
            stacked
                .iter()
                .map(|run| run.iter().map(|&(x, (base, top))| (x, top, base)).collect())
                .collect()
        } else {
            ai.iter()
                .map(|run| run.iter().map(|&(x, v)| (x, v, 0.)).collect())
                .collect()
        };
        let mut mesh = egui::Mesh::default();
        for run in bands.iter().filter(|run| run.len() > 1) {
            let points: Vec<(f32, f32, f32)> = run
                .iter()
                .map(|&(x, top, base)| {
                    (
                        plot_x(plot, x),
                        plot_y(plot, top, max),
                        plot_y(plot, base, max),
                    )
                })
                .collect();
            band(&mut mesh, &points, widgets::tone(color, alpha));
        }
        if !mesh.is_empty() {
            clip.add(Shape::mesh(mesh));
        }
        for run in &bands {
            let line: Vec<Pos2> = run.iter().map(|&(x, top, _)| at(x, top)).collect();
            if line.len() == 1 {
                clip.circle_filled(line[0], 2.5, color);
            } else {
                clip.add(Shape::line(line, Stroke::new(width, color)));
            }
        }
    }
    let label = format!(
        "{} {}: ai-memory e outros contêineres empilhados",
        metric.label(),
        if minutes == 1 {
            "no último minuto".to_owned()
        } else {
            format!("nos últimos {minutes} minutos")
        }
    );
    let response = ui.interact(plot, ui.id().with("resources-plot"), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Image, true, &label));
    if history.is_empty() {
        let text = "Aguardando amostras…";
        let galley = ui
            .painter()
            .layout_no_wrap(text.into(), Type::Body.font(), p.text2);
        let at = plot.center() - galley.size() / 2.;
        let rect = Rect::from_min_size(at, galley.size());
        ui.painter().galley(at, galley, p.text2);
        spoken(ui, rect, "waiting", text, None);
    } else if let Some(pointer) = response.hover_pos() {
        let x = |s: &Sample| plot_x(plot, fraction(history, minutes, s));
        if let Some(&s) = samples.iter().min_by(|a, b| {
            (x(a) - pointer.x)
                .abs()
                .total_cmp(&(x(b) - pointer.x).abs())
        }) {
            tooltip(ui, plot, x(s), s, metric);
        }
    }
    let end = ui
        .painter()
        .layout_no_wrap("agora".into(), FontId::proportional(11.), p.text2);
    let (axis, _) = ui.allocate_exact_size(vec2(width, end.size().y), Sense::hover());
    let painter = ui.painter();
    for (text, align, x) in [
        (
            format!("−{minutes} min"),
            egui::Align2::LEFT_TOP,
            plot.left(),
        ),
        (
            format!("−{} min", decimal_up_to(minutes as f64 / 2., 1)),
            egui::Align2::CENTER_TOP,
            plot.center().x,
        ),
        ("agora".to_owned(), egui::Align2::RIGHT_TOP, plot.right()),
    ] {
        painter.text(
            pos2(x, axis.top()),
            align,
            text,
            FontId::proportional(11.),
            p.text2,
        );
    }
}
/// A posição de uma amostra na janela (0 = início, 1 = a última).
fn fraction(history: &VecDeque<Sample>, minutes: i64, s: &Sample) -> f32 {
    let end = history.back().map_or(s.at, |last| last.at);
    let start = end - chrono::Duration::minutes(minutes);
    ((s.at - start).num_milliseconds() as f32 / (minutes * 60 * 1000) as f32).clamp(0., 1.)
}
/// A linha tracejada na amostra e a caixa com a hora, ai-memory, Outros e o
/// total do Docker (à esquerda da linha; à direita, se não couber).
fn tooltip(ui: &mut Ui, plot: Rect, x: f32, s: &Sample, metric: Metric) {
    let p = theme::palette(ui.visuals());
    ui.painter().extend(Shape::dashed_line(
        &[pos2(x, plot.top()), pos2(x, plot.bottom())],
        Stroke::new(1.0_f32, p.text2),
        3.,
        3.,
    ));
    let (ai, others, total) = stack(s, metric);
    let time = s.at.with_timezone(&Local).format("%H:%M:%S").to_string();
    let rows = [
        (Some(p.acc), "ai-memory", metric.amount(ai)),
        (Some(p.series2), "Outros", metric.amount(others)),
        (None, "Total Docker", metric.amount(total)),
    ];
    let font = FontId::proportional(12.);
    let strong = FontId::new(12., theme::semibold());
    let layout = |text: &str, font: &FontId, color: Color32| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), color)
    };
    let time_galley = layout(&time, &font, p.text2);
    let line = time_galley.size().y;
    let galleys: Vec<_> = rows
        .iter()
        .map(|(mark, label, value)| {
            (
                *mark,
                layout(label, &font, p.text),
                layout(value, &strong, p.text),
            )
        })
        .collect();
    let inner = galleys
        .iter()
        .map(|(mark, label, value)| {
            mark.map_or(0., |_| 14.) + label.size().x + 12. + value.size().x
        })
        .fold(time_galley.size().x, f32::max);
    let size = vec2((inner + 20.).max(170.), 8. + line * 4. + 3. * 3. + 4. + 8.);
    let mut left = x - 10. - size.x;
    if left < plot.left() {
        left = x + 10.;
    }
    let rect = Rect::from_min_size(pos2(left, plot.top() + 10.), size);
    let painter = ui.painter();
    painter.add(
        egui::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: p.shadow,
        }
        .as_shape(rect, 8.),
    );
    painter.rect(
        rect,
        8.,
        p.surf,
        Stroke::new(1.0_f32, p.line),
        StrokeKind::Inside,
    );
    let (l, r) = (rect.left() + 10., rect.right() - 10.);
    let mut y = rect.top() + 8.;
    painter.galley(pos2(l, y), time_galley, p.text2);
    y += line + 3.;
    for (i, (mark, label, value)) in galleys.into_iter().enumerate() {
        if i == 2 {
            painter.hline(l..=r, y + 0.5, Stroke::new(1.0_f32, p.line));
            y += 4.;
        }
        let mut text_left = l;
        if let Some(color) = mark {
            painter.rect_filled(
                Rect::from_center_size(pos2(l + 4., y + line / 2.), vec2(8., 8.)),
                2.,
                color,
            );
            text_left += 14.;
        }
        painter.galley(pos2(text_left, y), label, p.text);
        painter.galley(pos2(r - value.size().x, y), value, p.text);
        y += line + 3.;
    }
    let text = format!(
        "{time} · ai-memory {} · Outros {} · Total Docker {}",
        rows[0].2, rows[1].2, rows[2].2
    );
    spoken(ui, rect, "resources-tooltip", &text, None);
}
/// Quadrados de 10 × 10 (raio 3) e os nomes das séries (12, text2).
fn legend_width(ui: &Ui, items: &[(Color32, &str)]) -> f32 {
    items
        .iter()
        .map(|(_, text)| {
            10. + 6.
                + ui.painter()
                    .layout_no_wrap(
                        (*text).into(),
                        FontId::proportional(12.),
                        Color32::PLACEHOLDER,
                    )
                    .size()
                    .x
        })
        .sum::<f32>()
        + 14. * (items.len().saturating_sub(1)) as f32
}
fn legend_items(ui: &mut Ui, items: &[(Color32, &str)]) {
    let p = theme::palette(ui.visuals());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 14.;
        for (color, text) in items {
            let galley =
                ui.painter()
                    .layout_no_wrap((*text).into(), FontId::proportional(12.), p.text2);
            let (rect, response) = ui.allocate_exact_size(
                vec2(10. + 6. + galley.size().x, galley.size().y.max(10.)),
                Sense::hover(),
            );
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, *text));
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(rect.left(), rect.center().y - 5.), vec2(10., 10.)),
                3.,
                *color,
            );
            ui.painter().galley(
                pos2(rect.left() + 16., rect.center().y - galley.size().y / 2.),
                galley,
                p.text2,
            );
        }
    });
}

// ---- Linha de baixo ----

/// Contêineres em execução e handoffs recentes, lado a lado ou um embaixo do
/// outro.
fn bottom(ui: &mut Ui, d: &Data<'_>) -> Option<Go> {
    let mut go = None;
    let lines = flex_lines(ui.available_width(), &[(420., 3.), (300., 2.)], GAP);
    cards(
        ui,
        &["Contêineres em execução", "Handoffs recentes"],
        lines,
        egui::Margin::ZERO,
        |ui, i| {
            let next = if i == 0 {
                containers(ui, d.resources)
            } else {
                recents(ui, d.api, d.now).then_some(Go::Sessions)
            };
            if next.is_some() {
                go = next;
            }
        },
    );
    go
}
/// Título 13/600 com o complemento em text2 e o link à direita (12 × 16 de
/// margem, 8 embaixo). `true` no link.
fn card_head(ui: &mut Ui, title: &str, extra: &str, action: &str) -> bool {
    let p = theme::palette(ui.visuals());
    let width = ui.available_width();
    let action_width = ui
        .painter()
        .layout_no_wrap(action.into(), FontId::proportional(12.), p.acc)
        .size()
        .x;
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping::truncate_at_width(
            (width - 32. - action_width - 8.).max(0.),
        ),
        ..Default::default()
    };
    job.append(
        title,
        0.,
        egui::TextFormat::simple(FontId::new(13., theme::semibold()), p.text),
    );
    job.append(
        extra,
        6.,
        egui::TextFormat::simple(Type::Body.font(), p.text2),
    );
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 12. + galley.size().y + 8.), Sense::hover());
    let at = pos2(rect.left() + 16., rect.top() + 12.);
    let text_rect = Rect::from_min_size(at, galley.size());
    let y = text_rect.center().y;
    ui.painter().galley(at, galley, p.text);
    spoken(
        ui,
        text_rect,
        (title, "head"),
        &format!("{title} {extra}"),
        None,
    );
    link(ui, rect.right() - 16., y, action)
}
/// Uma mensagem no lugar das linhas (12, text2), numa linha de `ROW`.
fn note(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.visuals());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    let galley = widgets::one_line(ui, text, Type::Caption.font(), p.text2, width - 32.);
    let at = pos2(rect.left() + 16., rect.center().y - galley.size().y / 2.);
    let text_rect = Rect::from_min_size(at, galley.size());
    ui.painter().galley(at, galley, p.text2);
    spoken(ui, text_rect, ("note", text), text, None);
}
/// A tabela simples dos contêineres em execução (a ordem padrão do Docker),
/// em linhas de `ROW`; o clique numa linha abre o contêiner no Docker.
fn containers(ui: &mut Ui, r: &ResourceSnapshot) -> Option<Go> {
    let p = theme::palette(ui.visuals());
    ui.spacing_mut().item_spacing.y = 0.;
    let known = r.at.is_some() && r.docker_error.is_none();
    let mut running: Vec<_> = r
        .containers
        .iter()
        .filter(|c| c.state == "running")
        .collect();
    running.sort_by(|a, b| docker_ui::default_order(a, b));
    let count = if known {
        format!(
            "{} de {}",
            number(running.len()),
            number(r.containers.len())
        )
    } else {
        "—".into()
    };
    let mut go =
        card_head(ui, "Contêineres em execução", &count, "Ver todos →").then_some(Go::Docker(None));
    let width = ui.available_width();
    let (head, _) = ui.allocate_exact_size(vec2(width, HEAD_ROW), Sense::hover());
    let name_right = head.right() - CPU_COLUMN - MEMORY_COLUMN - TASKS_COLUMN;
    // Borda direita do texto de CPU, Memória e Tarefas.
    let rights = [
        name_right + CPU_COLUMN - 8.,
        name_right + CPU_COLUMN + MEMORY_COLUMN - 8.,
        head.right() - 16.,
    ];
    for (i, (title, right)) in [
        ("Nome", head.left() + 16.),
        ("CPU", rights[0]),
        ("Memória", rights[1]),
        ("Tarefas", rights[2]),
    ]
    .into_iter()
    .enumerate()
    {
        let galley = ui
            .painter()
            .layout_no_wrap(title.into(), FontId::proportional(11.), p.text2);
        let left = if i == 0 {
            right
        } else {
            right - galley.size().x
        };
        let at = pos2(left, head.center().y - galley.size().y / 2.);
        let rect = Rect::from_min_size(at, galley.size());
        ui.painter().galley(at, galley, p.text2);
        let hint = (title == "Tarefas").then(|| docker_ui::TASKS_HINT.to_owned());
        spoken(ui, rect, ("head", title), title, hint);
    }
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0_f32, p.line),
    );
    if r.at.is_none() {
        note(ui, "Aguardando a primeira coleta do Docker.");
    } else if r.docker_error.is_some() {
        note(ui, "Docker indisponível. Veja o aviso acima.");
    } else if running.is_empty() {
        note(ui, "Nenhum contêiner em execução.");
    }
    if !known {
        return go;
    }
    for c in running {
        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
        let usage = c.usage.as_ref();
        let unknown = || "—".to_owned();
        let cpu = usage.map_or_else(unknown, |u| format!("{}%", decimal(u.cpu, 1)));
        let memory = usage.map_or_else(unknown, |u| bytes(u.memory));
        let tasks = usage
            .and_then(|u| u.tasks)
            .map_or_else(unknown, |t| number(t as usize));
        let spoken = format!(
            "{} · CPU {cpu} · Memória {memory} · Tarefas {tasks}",
            c.name
        );
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &spoken));
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, 0., widgets::tone(p.surf2, 0.6));
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.line),
        );
        let y = rect.center().y;
        ui.painter()
            .circle_filled(pos2(rect.left() + 16. + 3.5, y), 3.5, p.acc);
        let left = rect.left() + 16. + 7. + 8.;
        let room = name_right - 8. - left;
        let name = widgets::one_line(ui, &c.name, Type::Body.font(), p.text, room);
        let image_left = left + name.size().x + 8.;
        let image = widgets::one_line(
            ui,
            &c.image,
            Type::Caption.font(),
            p.text2,
            name_right - 8. - image_left,
        );
        let cut = name.elided || image.elided;
        let name_width = name.size().x;
        ui.painter()
            .galley(pos2(left, y - name.size().y / 2.), name, p.text);
        if left + name_width + 8. < name_right - 8. {
            ui.painter()
                .galley(pos2(image_left, y - image.size().y / 2.), image, p.text2);
        }
        let value_color = if usage.is_some() { p.text } else { p.text2 };
        for (text, right, color) in [
            (&cpu, rights[0], value_color),
            (&memory, rights[1], value_color),
            (&tasks, rights[2], p.text2),
        ] {
            let galley = ui
                .painter()
                .layout_no_wrap(text.clone(), Type::Body.font(), color);
            ui.painter().galley(
                pos2(right - galley.size().x, y - galley.size().y / 2.),
                galley,
                color,
            );
        }
        let response = if cut {
            response.on_hover_text(format!("{}\n{}", c.name, c.image))
        } else {
            response
        };
        if response.clicked() {
            go = Some(Go::Docker(Some(c.id.clone())));
        }
    }
    go
}
/// Os 4 handoffs mais recentes: projeto e idade, e o título do resumo.
/// `true` em "Abrir →".
fn recents(ui: &mut Ui, api: &ApiSnapshot, now: DateTime<Utc>) -> bool {
    let p = theme::palette(ui.visuals());
    ui.spacing_mut().item_spacing.y = 0.;
    let count = match api.open_handoffs().map(|(_, text)| text) {
        Some(n) if n == "1" => "1 aberto".to_owned(),
        Some(n) => format!("{n} abertos"),
        None => "—".to_owned(),
    };
    let open = card_head(ui, "Handoffs recentes", &count, "Abrir →");
    if api.at.is_none() {
        note(ui, "Aguardando a consulta ao ai-memory.");
    } else if api.handoffs.is_empty() {
        note(ui, "Nenhum handoff aberto.");
    }
    let width = ui.available_width();
    for h in api.handoffs.iter().take(RECENT) {
        let age = sessions_ui::age(&h.at, now)
            .map_or_else(|| "—".to_owned(), |a| a.long().unwrap_or_else(|| a.short()));
        let (summary, _) = sessions_ui::headline(h);
        let age_galley = ui
            .painter()
            .layout_no_wrap(age.clone(), Type::Caption.font(), p.text2);
        let project = widgets::one_line(
            ui,
            &h.scope,
            Type::Body.font(),
            p.text,
            width - 32. - age_galley.size().x - 8.,
        );
        let preview = widgets::one_line(ui, &summary, Type::Caption.font(), p.text2, width - 32.);
        let height = 7. + project.size().y + 1. + preview.size().y + 7.;
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        ui.painter().hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(1.0_f32, p.line),
        );
        let top = rect.top() + 7.;
        let cut = project.elided || preview.elided;
        let project_height = project.size().y;
        ui.painter().galley(
            pos2(
                rect.right() - 16. - age_galley.size().x,
                top + (project_height - age_galley.size().y) / 2.,
            ),
            age_galley,
            p.text2,
        );
        ui.painter()
            .galley(pos2(rect.left() + 16., top), project, p.text);
        ui.painter().galley(
            pos2(rect.left() + 16., top + project_height + 1.),
            preview,
            p.text2,
        );
        let text = format!("{} · {age} · {summary}", h.scope);
        let tip = cut.then(|| format!("{}\n{summary}", h.scope));
        spoken(ui, rect, ("recent", &h.scope, &h.id), &text, tip);
    }
    ui.add_space(6.);
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::{Error, Scope},
        model::{Container, Handoff, Project},
        widgets::tests::{THEMES, click_frames, context, node, nodes, rect_of, run_sized, shapes},
    };
    use egui::{Event, accesskit::Role, epaint::ColorMode};
    use serde_json::json;

    /// 1100 − 16 de margem = 1084: os quatro indicadores e a linha de baixo
    /// lado a lado.
    const WIDE: [f32; 2] = [1100., 1000.];
    /// 610 − 16 = 594, o conteúdo de 720 no macOS: dois por linha.
    const NARROW: [f32; 2] = [610., 1400.];
    const M: u64 = 1 << 20;

    fn running(name: &str, cpu: f64, mib: u64, tasks: u64) -> Container {
        Container {
            id: format!("{name}-8f3c21a9d0e4"),
            name: name.into(),
            image: format!("{name}:latest"),
            state: "running".into(),
            usage: Some(Usage {
                cpu,
                memory: mib * M,
                limit: 8 << 30,
                tasks: Some(tasks),
            }),
            ..Default::default()
        }
    }
    fn stopped(name: &str) -> Container {
        Container {
            id: format!("{name}-8f3c21a9d0e4"),
            name: name.into(),
            state: "exited".into(),
            ..Default::default()
        }
    }
    /// Os oito do artboard; três em execução (15,5% · 733 MiB).
    fn resources(now: DateTime<Utc>) -> ResourceSnapshot {
        ResourceSnapshot {
            at: Some(now),
            containers: vec![
                stopped("minio"),
                running("redis-cache", 0.4, 94, 6),
                running("postgres-dev", 2.7, 218, 12),
                running("ai-memory", 12.4, 421, 46),
                stopped("grafana"),
                stopped("loki"),
                stopped("tempo"),
                stopped("zipkin"),
            ],
            ..Default::default()
        }
    }
    fn usage(cpu: f64, mib: u64) -> Usage {
        Usage {
            cpu,
            memory: mib * M,
            ..Default::default()
        }
    }
    fn sample(
        now: DateTime<Utc>,
        ago: i64,
        ai: Option<(f64, u64)>,
        docker: Option<(f64, u64)>,
    ) -> Sample {
        Sample {
            at: now - chrono::Duration::seconds(ago),
            ai: ai.map(|(c, m)| usage(c, m)),
            // O cenário padrão: o ai-memory é o contêiner monitorado.
            ai_in_docker: ai.is_some(),
            docker: docker.map(|(c, m)| usage(c, m)),
        }
    }
    /// Uma amostra isolada há 4 min e três seguidas no último minuto.
    fn history(now: DateTime<Utc>) -> VecDeque<Sample> {
        VecDeque::from([
            sample(now, 240, Some((49.4, 500)), Some((60., 900))),
            sample(now, 20, Some((9., 400)), Some((12., 700))),
            sample(now, 10, Some((10., 436)), Some((13., 750))),
            sample(now, 0, Some((10.4, 421)), Some((15.5, 733))),
        ])
    }
    fn handoff(now: DateTime<Utc>, scope: &str, minutes: i64, summary: Option<&str>) -> Handoff {
        Handoff {
            id: format!("{scope}-{minutes}"),
            agent: "claude-code".into(),
            at: (now - chrono::Duration::minutes(minutes)).to_rfc3339(),
            state: "open".into(),
            summary: summary.map(str::to_owned),
            scope: scope.into(),
            ..Default::default()
        }
    }
    const MESSAGE: &str = "Started: <cross-session-message from=\"local_0001\" from-name=\"Trilha\">\nCritério de desempate\n\nMais texto.\n</cross-session-message>";
    fn api(now: DateTime<Utc>) -> ApiSnapshot {
        let mut restricted = handoff(now, "acme / infra", 60 * 26, Some("segredo"));
        restricted.redacted = true;
        ApiSnapshot {
            at: Some(now),
            handoff_total: Some(75),
            handoffs: vec![
                handoff(now, "acme / api-pagamentos", 12, Some(MESSAGE)),
                handoff(
                    now,
                    "acme / portal-web",
                    38,
                    Some("## Migração concluída\n\nFaltam testes E2E."),
                ),
                handoff(now, "pessoal / notas", 120, None),
                restricted,
                handoff(now, "acme / quinto", 60 * 30, Some("Não aparece")),
            ],
            projects: (0..14)
                .map(|i| Project {
                    open_session_count: Some(u64::from(i < 11)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    fn scope() -> Scope {
        Scope {
            workspace: "ws".into(),
            project: "proj".into(),
        }
    }
    fn healthy(now: DateTime<Utc>) -> Snapshot {
        Snapshot {
            at: Some(now),
            status: Some(json!({
                "version": "3.3.1",
                "counts": {},
                "consolidation": {"completed": 116, "pending": 0, "running": 0, "failed": 0},
                "write_queue": [0, 1024],
                "providers": {"llm": {"status": "ok"}, "embedding": {"status": "ok"}},
            })),
            pending: Some(3),
            pending_scope: Some(scope()),
            ..Default::default()
        }
    }

    /// A página num contexto, com os dados e o relógio fixos.
    struct Page {
        ctx: egui::Context,
        size: [f32; 2],
        overview: Overview,
        resources: ResourceSnapshot,
        history: VecDeque<Sample>,
        api: ApiSnapshot,
        health: Snapshot,
        config: Config,
        now: DateTime<Utc>,
        go: Vec<Go>,
    }
    impl Page {
        fn new(theme: egui::Theme, size: [f32; 2]) -> Self {
            let now = Utc::now();
            Self {
                ctx: context(theme),
                size,
                overview: Overview::default(),
                resources: resources(now),
                history: history(now),
                api: api(now),
                health: healthy(now),
                config: Config {
                    container: "ai-memory".into(),
                    proposal_watch: Some(scope()),
                    ..Default::default()
                },
                now,
                go: Vec::new(),
            }
        }
        fn data(&self) -> Data<'_> {
            Data {
                resources: &self.resources,
                history: &self.history,
                api: &self.api,
                health: &self.health,
                config: &self.config,
                now: self.now,
            }
        }
        fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
            let Self {
                ctx,
                size,
                overview,
                resources,
                history,
                api,
                health,
                config,
                now,
                go,
            } = self;
            let data = Data {
                resources,
                history,
                api,
                health,
                config,
                now: *now,
            };
            run_sized(ctx, *size, events, |ui| {
                if let Some(next) = overview.show(ui, &data) {
                    go.push(next);
                }
            })
        }
        fn show(&mut self) -> egui::FullOutput {
            self.frame(vec![])
        }
        fn click(&mut self, pos: Pos2) -> egui::FullOutput {
            let [arrive, press, release] = click_frames(pos);
            self.frame(arrive);
            self.frame(press);
            self.frame(release)
        }
        fn click_on(&mut self, role: Role, text: &str) -> egui::FullOutput {
            let out = self.show();
            self.click(rect_of(&node(&out, role, text)).center())
        }
        /// O ponteiro parado em `pos`; devolve o 2º frame.
        fn hover(&mut self, pos: Pos2) -> egui::FullOutput {
            self.frame(vec![Event::PointerMoved(pos)]);
            self.frame(vec![Event::PointerMoved(pos)])
        }
        /// Além do atraso do tooltip do egui (1/60 s por frame).
        fn rest(&mut self, pos: Pos2) -> egui::FullOutput {
            let mut out = self.frame(vec![Event::PointerMoved(pos)]);
            for _ in 0..45 {
                out = self.show();
            }
            out
        }
    }
    /// Os textos (rótulo ou valor) dos nós `role` dentro de `rect`, de cima
    /// para baixo e da esquerda para a direita.
    fn inside(out: &egui::FullOutput, role: Role, rect: Rect) -> Vec<String> {
        let mut found: Vec<(f32, f32, String)> = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == role && n.bounds().is_some())
            .filter_map(|(_, n)| {
                let r = rect_of(n);
                let text = n.label().or(n.value())?.to_owned();
                rect.expand(0.5)
                    .contains_rect(r)
                    .then_some((r.top(), r.left(), text))
            })
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        found.into_iter().map(|(_, _, t)| t).collect()
    }
    fn pane(out: &egui::FullOutput, title: &str) -> Rect {
        rect_of(&node(out, Role::Pane, title))
    }
    fn has(out: &egui::FullOutput, role: Role, text: &str) -> bool {
        !nodes(out, role, text).is_empty()
    }

    #[test]
    fn cards_on_a_line_take_the_height_of_the_tallest_in_both_kinds_of_line() {
        let titles = ["Um", "Dois", "Três", "Quatro"];
        // Conteúdos de alturas diferentes: 20, 50, 80 e 110.
        let content = |i: usize| 20. + 30. * i as f32;
        let kpi = egui::Margin {
            left: 16,
            right: 16,
            top: 14,
            bottom: 12,
        };
        // (caso, linhas, margem): os indicadores em 2 + 2 e a linha de baixo.
        for (case, lines, margin) in [
            ("kpis", kpi_lines(594.), kpi),
            (
                "bottom",
                flex_lines(1084., &[(420., 3.), (300., 2.)], GAP),
                egui::Margin::ZERO,
            ),
        ] {
            let count: usize = lines.iter().map(Vec::len).sum();
            let ctx = context(egui::Theme::Dark);
            let out = run_sized(&ctx, WIDE, vec![], |ui| {
                cards(ui, &titles[..count], lines.clone(), margin, |ui, i| {
                    ui.allocate_exact_size(vec2(10., content(i)), Sense::hover());
                });
            });
            for line in &lines {
                let heights: Vec<f32> = line
                    .iter()
                    .map(|&(i, _)| pane(&out, titles[i]).height())
                    .collect();
                let sizes: Vec<f32> = line.iter().map(|&(i, _)| content(i)).collect();
                let tallest = sizes.iter().copied().fold(0., f32::max);
                assert!(
                    sizes.iter().any(|&s| s < tallest),
                    "{case}: precondition, the contents differ: {sizes:?}"
                );
                assert!(
                    heights.iter().all(|h| (h - heights[0]).abs() < 0.5),
                    "{case}: same line, same height: {heights:?}"
                );
                // O mais baixo cresceu até o conteúdo mais alto e as margens.
                let room = tallest + f32::from(margin.top) + f32::from(margin.bottom);
                assert!(heights[0] >= room, "{case}: {heights:?} / {room}");
            }
        }
    }

    #[test]
    fn flex_lines_wrap_like_css_flex_wrap() {
        let kpis = [(220., 1.); 4];
        let bottom = [(420., 3.), (300., 2.)];
        type Lines = Vec<Vec<(usize, f32)>>;
        type Case<'a> = (&'a [(f32, f32)], f32, Lines);
        // (itens, largura, linhas esperadas)
        let cases: [Case<'_>; 11] = [
            (
                &kpis,
                594.,
                vec![vec![(0, 291.), (1, 291.)], vec![(2, 291.), (3, 291.)]],
            ),
            (
                &kpis,
                683.9,
                vec![
                    vec![(0, 335.95), (1, 335.95)],
                    vec![(2, 335.95), (3, 335.95)],
                ],
            ),
            // 3 × 220 + 2 × 12: três numa linha e o quarto sozinho, esticado.
            (
                &kpis,
                684.,
                vec![vec![(0, 220.), (1, 220.), (2, 220.)], vec![(3, 684.)]],
            ),
            (
                &kpis,
                916.,
                vec![vec![(0, 220.), (1, 220.), (2, 220.), (3, 220.)]],
            ),
            (
                &kpis,
                1040.,
                vec![vec![(0, 251.), (1, 251.), (2, 251.), (3, 251.)]],
            ),
            (
                &kpis,
                451.9,
                vec![
                    vec![(0, 451.9)],
                    vec![(1, 451.9)],
                    vec![(2, 451.9)],
                    vec![(3, 451.9)],
                ],
            ),
            (
                &kpis,
                0.,
                vec![vec![(0, 0.)], vec![(1, 0.)], vec![(2, 0.)], vec![(3, 0.)]],
            ),
            (&bottom, 594., vec![vec![(0, 594.)], vec![(1, 594.)]]),
            (&bottom, 731.9, vec![vec![(0, 731.9)], vec![(1, 731.9)]]),
            (&bottom, 732., vec![vec![(0, 420.), (1, 300.)]]),
            // Sobra de 308 na proporção 3 : 2.
            (&bottom, 1040., vec![vec![(0, 604.8), (1, 423.2)]]),
        ];
        for (items, width, want) in cases {
            let got = flex_lines(width, items, GAP);
            let shape = |l: &Lines| {
                l.iter()
                    .map(|line| line.iter().map(|&(i, _)| i).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            };
            assert_eq!(shape(&got), shape(&want), "{width}");
            for (g, w) in got.iter().flatten().zip(want.iter().flatten()) {
                assert!((g.1 - w.1).abs() < 0.01, "{width}: {got:?} / {want:?}");
            }
        }
    }

    #[test]
    fn indicators_are_four_or_two_by_two_or_one_per_line_never_three_and_one() {
        // (largura, índices por linha, largura de cada um)
        let cases: [(f32, &[&[usize]], f32); 9] = [
            (0., &[&[0], &[1], &[2], &[3]], 0.),
            (451.9, &[&[0], &[1], &[2], &[3]], 451.9),
            // 2 × 220 + 12.
            (452., &[&[0, 1], &[2, 3]], 220.),
            (594., &[&[0, 1], &[2, 3]], 291.),
            // Onde o flex-wrap dava 3 + 1 (684–915,9): 2 + 2.
            (684., &[&[0, 1], &[2, 3]], 336.),
            (800., &[&[0, 1], &[2, 3]], 394.),
            (915.9, &[&[0, 1], &[2, 3]], 451.95),
            // 4 × 220 + 3 × 12.
            (916., &[&[0, 1, 2, 3]], 220.),
            (1040., &[&[0, 1, 2, 3]], 251.),
        ];
        for (width, want, each) in cases {
            let got = kpi_lines(width);
            let shape: Vec<Vec<usize>> = got
                .iter()
                .map(|line| line.iter().map(|&(i, _)| i).collect())
                .collect();
            assert_eq!(shape, want, "{width}");
            assert!(
                got.iter().flatten().all(|&(_, w)| (w - each).abs() < 0.01),
                "{width}: {got:?}"
            );
        }
    }

    #[test]
    fn indicators_go_two_by_two_on_the_page_where_three_would_fit() {
        for (theme, _) in THEMES {
            // 816 − 16 = 800 de conteúdo: cabem três de 220, não quatro.
            let mut page = Page::new(theme, [816., 1400.]);
            let out = page.show();
            let kpis: Vec<Rect> = [
                "ai-memory · CPU",
                "ai-memory · Memória",
                "Contêineres",
                "Continuidade",
            ]
            .iter()
            .map(|t| pane(&out, t))
            .collect();
            let (left, right) = (8., 816. - 8.);
            for line in [&kpis[..2], &kpis[2..]] {
                assert!(
                    (line[0].left() - left).abs() < 0.5
                        && (line[1].right() - right).abs() < 0.5
                        && (line[1].left() - line[0].right() - GAP).abs() < 0.5
                        && line[0].top() == line[1].top()
                        && line[0].size() == line[1].size(),
                    "{theme:?}: {line:?}"
                );
            }
            assert!(
                kpis[2].top() >= kpis[0].bottom() + GAP - 0.5,
                "{theme:?}: the second pair is a line of its own: {kpis:?}"
            );
            assert_eq!(kpis[0].width(), kpis[3].width(), "{theme:?}");
        }
    }

    #[test]
    fn health_strip_names_every_state_with_its_tone_in_both_themes() {
        let now = Utc::now();
        let alerts = Snapshot {
            status: Some(json!({
                "version": "3.3.1",
                "counts": {},
                "consolidation": {"completed": 1, "pending": 0, "running": 0, "failed": 2},
                "write_queue": [900, 1024],
            })),
            ..healthy(now)
        };
        let forbidden = Error::Forbidden.to_string();
        // (caso, leitura, título, resumo, tom)
        type Case = (
            &'static str,
            Snapshot,
            &'static str,
            String,
            fn(&theme::Palette) -> Color32,
        );
        let cases: [Case; 5] = [
            (
                "waiting",
                Snapshot::default(),
                "Aguardando",
                "A primeira coleta de saúde ainda não terminou.".into(),
                |p| p.text2,
            ),
            (
                "stale",
                Snapshot {
                    at: Some(now - chrono::Duration::seconds(200)),
                    ..healthy(now)
                },
                "Leitura atrasada",
                "A última coleta tem mais de 90 s.".into(),
                |p| p.warn,
            ),
            (
                "unavailable",
                Snapshot {
                    at: Some(now),
                    error: Some(Error::Forbidden),
                    ..Default::default()
                },
                "Indisponível",
                forbidden,
                |p| p.bad,
            ),
            (
                "alerts",
                alerts,
                "Com alertas",
                "Consolidações com falha · Fila de escrita acima de 80%".into(),
                |p| p.warn,
            ),
            (
                "healthy",
                healthy(now),
                "Tudo funcionando",
                "ai-memory 3.3.1 · providers ok · consolidação sem pendências · fila de escrita 0 de 1.024".into(),
                |p| p.acc,
            ),
        ];
        for (theme, p) in THEMES {
            for (case, health, title, summary, tone) in &cases {
                let mut page = Page::new(theme, WIDE);
                page.health = health.clone();
                let out = page.show();
                let strip = pane(&out, "Saúde");
                assert_eq!(
                    inside(&out, Role::Label, strip),
                    [title.to_string(), summary.clone()],
                    "{theme:?} {case}"
                );
                assert_eq!(inside(&out, Role::Link, strip), ["Ver saúde →"]);
                let color = tone(p);
                let icon = shapes(&out).into_iter().any(|s| match s {
                    Shape::Path(path) => {
                        path.stroke.color == ColorMode::Solid(color)
                            && path.points.iter().all(|&pt| strip.contains(pt))
                    }
                    _ => false,
                });
                assert!(icon, "{theme:?} {case}: icon in {color:?}");
                let others = [p.acc, p.warn, p.bad, p.text2]
                    .into_iter()
                    .filter(|&c| c != color)
                    .any(|c| {
                        shapes(&out).into_iter().any(|s| match s {
                            Shape::Path(path) => {
                                path.stroke.color == ColorMode::Solid(c)
                                    && path.points.iter().all(|&pt| strip.contains(pt))
                            }
                            _ => false,
                        })
                    });
                assert!(!others, "{theme:?} {case}: only its own tone");
            }
            let mut page = Page::new(theme, WIDE);
            page.click_on(Role::Link, "Ver saúde →");
            assert_eq!(page.go, [Go::Health], "{theme:?}");
        }
    }

    #[test]
    fn healthy_summary_lists_only_what_is_known() {
        let with = |status: Value, version: &str| Snapshot {
            status: Some(status),
            identity: Some(health::Identity {
                version: version.into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        // (caso, leitura, fatos)
        let cases: [(&str, Snapshot, &[&str]); 12] = [
            (
                "complete",
                healthy(Utc::now()),
                &[
                    "ai-memory 3.3.1",
                    "providers ok",
                    "consolidação sem pendências",
                    "fila de escrita 0 de 1.024",
                ],
            ),
            ("nothing", with(json!({}), ""), &[]),
            (
                "identity version only",
                with(json!({}), "3.2.0"),
                &["ai-memory 3.2.0"],
            ),
            (
                "status version first",
                with(json!({"version": "3.3.1"}), "3.2.0"),
                &["ai-memory 3.3.1"],
            ),
            (
                "one provider unknown",
                with(json!({"providers": {"llm": {"status": "ok"}}}), ""),
                &[],
            ),
            (
                "one provider neutral",
                with(
                    json!({"providers": {"llm": {"status": "ok"}, "embedding": {"status": "degraded"}}}),
                    "",
                ),
                &[],
            ),
            (
                "healthy providers",
                with(
                    json!({"providers": {"llm": {"status": "healthy"}, "embedding": {"status": "ok"}}}),
                    "",
                ),
                &["providers ok"],
            ),
            (
                "one pending",
                with(json!({"consolidation": {"pending": 1}}), ""),
                &["consolidação: 1 pendente"],
            ),
            (
                "many pending",
                with(json!({"consolidation": {"pending": 1234}}), ""),
                &["consolidação: 1.234 pendentes"],
            ),
            (
                "pending unknown",
                with(json!({"consolidation": {"running": 0}}), ""),
                &[],
            ),
            (
                "capacity unknown",
                with(json!({"write_queue": [5]}), ""),
                &[],
            ),
            (
                "queue",
                with(json!({"write_queue": [12, 1024]}), ""),
                &["fila de escrita 12 de 1.024"],
            ),
        ];
        for (case, s, want) in cases {
            assert_eq!(facts(&s), want, "{case}");
        }
        assert_eq!(
            strip_text(&with(json!({}), ""), &Overall::Healthy),
            (
                "Tudo funcionando",
                "Sem alertas nas verificações disponíveis.".to_owned()
            ),
            "nothing known: no empty line"
        );
    }

    /// Título, valor e linha de baixo de cada indicador.
    fn card_texts(out: &egui::FullOutput) -> Vec<Vec<String>> {
        [
            "ai-memory · CPU",
            "ai-memory · Memória",
            "Contêineres",
            "Continuidade",
        ]
        .iter()
        .map(|title| inside(out, Role::Label, pane(out, title)))
        .collect()
    }

    #[test]
    fn cards_show_a_dash_never_zero_without_data_in_both_themes() {
        let now = Utc::now();
        let down = ResourceSnapshot {
            at: Some(now),
            docker_error: Some("Docker CLI não encontrado.".into()),
            ..Default::default()
        };
        let mut stopped_ai = resources(now);
        stopped_ai.containers[3] = stopped("ai-memory");
        let process = crate::model::HostProcess {
            pid: 77,
            name: "ai-memory".into(),
            group: "ai-memory".into(),
            usage: usage(50., 1024),
        };
        let native = ResourceSnapshot {
            processes: vec![process.clone()],
            ..stopped_ai.clone()
        };
        // Sem contêiner do ai-memory na lista: só o nativo.
        let mut only_native = native.clone();
        only_native.containers.remove(3);
        let down_native = ResourceSnapshot {
            processes: vec![process],
            ..down.clone()
        };
        // (caso, recursos, histórico, API, saúde, os quatro cartões)
        let cases = [
            (
                "everything",
                resources(now),
                history(now),
                api(now),
                healthy(now),
                [
                    [
                        "ai-memory · CPU",
                        "12,4 %",
                        "100% = 1 núcleo · média 5 min 19,7%",
                    ],
                    [
                        "ai-memory · Memória",
                        "421,0 MiB",
                        "pico em 5 min: 500,0 MiB",
                    ],
                    [
                        "Contêineres",
                        "3 de 8 ativos",
                        "demais contêineres: 3,1% · 312,0 MiB",
                    ],
                    [
                        "Continuidade",
                        "75 handoffs abertos",
                        "11 sessões abertas · 3 propostas",
                    ],
                ],
            ),
            (
                "nothing collected",
                ResourceSnapshot::default(),
                VecDeque::new(),
                ApiSnapshot::default(),
                Snapshot::default(),
                [
                    ["ai-memory · CPU", "—", "100% = 1 núcleo · média 5 min —"],
                    ["ai-memory · Memória", "—", "pico em 5 min: —"],
                    ["Contêineres", "—", "demais contêineres: —"],
                    ["Continuidade", "—", "— sessões abertas · — propostas"],
                ],
            ),
            (
                "docker down",
                down,
                VecDeque::from([sample(now, 0, None, None)]),
                api(now),
                healthy(now),
                [
                    ["ai-memory · CPU", "—", "100% = 1 núcleo · média 5 min —"],
                    ["ai-memory · Memória", "—", "pico em 5 min: —"],
                    ["Contêineres", "—", "demais contêineres: —"],
                    [
                        "Continuidade",
                        "75 handoffs abertos",
                        "11 sessões abertas · 3 propostas",
                    ],
                ],
            ),
            (
                "ai-memory stopped: the others are the whole Docker",
                stopped_ai.clone(),
                history(now),
                api(now),
                healthy(now),
                [
                    [
                        "ai-memory · CPU",
                        "—",
                        "100% = 1 núcleo · média 5 min 19,7%",
                    ],
                    ["ai-memory · Memória", "—", "pico em 5 min: 500,0 MiB"],
                    [
                        "Contêineres",
                        "2 de 8 ativos",
                        "demais contêineres: 3,1% · 312,0 MiB",
                    ],
                    [
                        "Continuidade",
                        "75 handoffs abertos",
                        "11 sessões abertas · 3 propostas",
                    ],
                ],
            ),
            // O nativo não está no total do Docker: nada a subtrair, mesmo
            // acima do total.
            (
                "stopped container, native running: the others are the whole Docker",
                native,
                history(now),
                api(now),
                healthy(now),
                [
                    [
                        "ai-memory · CPU",
                        "50,0 %",
                        "100% = 1 núcleo · média 5 min 19,7%",
                    ],
                    ["ai-memory · Memória", "1,0 GiB", "pico em 5 min: 500,0 MiB"],
                    [
                        "Contêineres",
                        "2 de 8 ativos",
                        "demais contêineres: 3,1% · 312,0 MiB",
                    ],
                    [
                        "Continuidade",
                        "75 handoffs abertos",
                        "11 sessões abertas · 3 propostas",
                    ],
                ],
            ),
            (
                "native only: the others are the whole Docker",
                only_native,
                history(now),
                api(now),
                healthy(now),
                [
                    [
                        "ai-memory · CPU",
                        "50,0 %",
                        "100% = 1 núcleo · média 5 min 19,7%",
                    ],
                    ["ai-memory · Memória", "1,0 GiB", "pico em 5 min: 500,0 MiB"],
                    [
                        "Contêineres",
                        "2 de 7 ativos",
                        "demais contêineres: 3,1% · 312,0 MiB",
                    ],
                    [
                        "Continuidade",
                        "75 handoffs abertos",
                        "11 sessões abertas · 3 propostas",
                    ],
                ],
            ),
            (
                "docker down, native running: no others",
                down_native,
                VecDeque::from([sample(now, 0, None, None)]),
                api(now),
                healthy(now),
                [
                    [
                        "ai-memory · CPU",
                        "50,0 %",
                        "100% = 1 núcleo · média 5 min —",
                    ],
                    ["ai-memory · Memória", "1,0 GiB", "pico em 5 min: —"],
                    ["Contêineres", "—", "demais contêineres: —"],
                    [
                        "Continuidade",
                        "75 handoffs abertos",
                        "11 sessões abertas · 3 propostas",
                    ],
                ],
            ),
        ];
        for (theme, _) in THEMES {
            for size in [WIDE, NARROW] {
                for (case, r, h, a, health, want) in &cases {
                    let mut page = Page::new(theme, size);
                    page.resources = r.clone();
                    page.history = h.clone();
                    page.api = a.clone();
                    page.health = health.clone();
                    assert_eq!(
                        card_texts(&page.show()),
                        want.map(|card| card.map(str::to_owned).to_vec()),
                        "{theme:?} {size:?} {case}"
                    );
                }
            }
        }
    }

    #[test]
    fn continuity_counts_handoffs_sessions_and_proposals() {
        let now = Utc::now();
        let base = Page::new(egui::Theme::Dark, WIDE);
        let continuity = |api: &ApiSnapshot, health: &Snapshot, watch: Option<Scope>| {
            let config = Config {
                proposal_watch: watch,
                ..base.config.clone()
            };
            let data = Data {
                api,
                health,
                config: &config,
                ..base.data()
            };
            let [.., k] = kpi_list(&data, 5);
            (k.value.map(|(v, u)| format!("{v} {u}")), k.sub)
        };
        let handoffs =
            |at: Option<DateTime<Utc>>, total: Option<usize>, listed: usize| ApiSnapshot {
                at,
                handoff_total: total,
                handoffs: vec![Handoff::default(); listed],
                ..api(now)
            };
        // Handoffs: (caso, API, valor)
        for (case, a, want) in [
            (
                "server total",
                handoffs(Some(now), Some(75), 3),
                Some("75 handoffs abertos"),
            ),
            (
                "one",
                handoffs(Some(now), Some(1), 1),
                Some("1 handoff aberto"),
            ),
            (
                "zero",
                handoffs(Some(now), Some(0), 0),
                Some("0 handoffs abertos"),
            ),
            (
                "list only",
                handoffs(Some(now), None, 200),
                Some("200+ handoffs abertos"),
            ),
            (
                "one listed only",
                handoffs(Some(now), None, 1),
                Some("1+ handoffs abertos"),
            ),
            (
                "before the 1st collection",
                handoffs(None, Some(5), 5),
                None,
            ),
        ] {
            let (value, _) = continuity(&a, &healthy(now), Some(scope()));
            assert_eq!(value.as_deref(), want, "{case}");
        }
        // Sessões abertas: (caso, contagens por projeto, texto)
        for (case, counts, want) in [
            ("many", vec![Some(4), Some(7)], "11 sessões abertas"),
            ("one", vec![Some(1), Some(0)], "1 sessão aberta"),
            ("zero", vec![Some(0)], "0 sessões abertas"),
            ("no projects", vec![], "0 sessões abertas"),
            (
                "a project without the count",
                vec![Some(4), None],
                "— sessões abertas",
            ),
        ] {
            let a = ApiSnapshot {
                projects: counts
                    .into_iter()
                    .map(|open_session_count| Project {
                        open_session_count,
                        ..Default::default()
                    })
                    .collect(),
                ..api(now)
            };
            let (_, sub) = continuity(&a, &healthy(now), None);
            assert_eq!(sub, want, "{case}");
        }
        // Propostas: (caso, vigiado?, leitura, trecho)
        let pending = |n: Option<usize>| Snapshot {
            pending: n,
            ..healthy(now)
        };
        let other = Snapshot {
            pending_scope: Some(Scope {
                workspace: "ws".into(),
                project: "outro".into(),
            }),
            ..healthy(now)
        };
        let stale = Snapshot {
            at: Some(now - chrono::Duration::seconds(200)),
            ..healthy(now)
        };
        for (case, watch, health, want) in [
            ("no watched project", false, pending(Some(3)), ""),
            ("three", true, pending(Some(3)), " · 3 propostas"),
            ("one", true, pending(Some(1)), " · 1 proposta"),
            ("zero", true, pending(Some(0)), " · 0 propostas"),
            ("list limit", true, pending(Some(250)), " · 200+ propostas"),
            ("refused", true, pending(None), " · — propostas"),
            ("another project's reading", true, other, " · — propostas"),
            ("stale reading", true, stale, " · — propostas"),
        ] {
            let (_, sub) = continuity(&api(now), &health, watch.then(scope));
            assert_eq!(sub, format!("11 sessões abertas{want}"), "{case}");
        }
    }

    #[test]
    fn average_and_peak_follow_the_chosen_window() {
        let page = Page::new(egui::Theme::Dark, WIDE);
        for (minutes, cpu, memory) in [
            (
                1,
                "100% = 1 núcleo · média 1 min 9,8%",
                "pico em 1 min: 436,0 MiB",
            ),
            (
                5,
                "100% = 1 núcleo · média 5 min 19,7%",
                "pico em 5 min: 500,0 MiB",
            ),
            (
                10,
                "100% = 1 núcleo · média 10 min 19,7%",
                "pico em 10 min: 500,0 MiB",
            ),
        ] {
            let [c, m, ..] = kpi_list(&page.data(), minutes);
            assert_eq!((c.sub.as_str(), m.sub.as_str()), (cpu, memory), "{minutes}");
        }
    }

    /// O retângulo da sparkline de `card` (borda 1 e margem 16 × 14).
    fn spark_rect(out: &egui::FullOutput, card: &str) -> Rect {
        let pane = pane(out, card);
        let right = pane.right() - 1. - 16.;
        let top = pane.top() + 1. + 14.;
        Rect::from_min_max(
            pos2(right - SPARK_WIDTH, top),
            pos2(right, top + SPARK_HEIGHT),
        )
    }
    /// Linhas e pontos acc pintados na sparkline de `card`.
    fn spark(out: &egui::FullOutput, card: &str, p: &theme::Palette) -> (Vec<usize>, usize) {
        let rect = spark_rect(out, card);
        let mut lines = Vec::new();
        let mut dots = 0;
        for s in shapes(out) {
            match s {
                Shape::Path(path)
                    if path.stroke.color == ColorMode::Solid(p.acc)
                        && path.points.iter().all(|&pt| rect.contains(pt)) =>
                {
                    lines.push(path.points.len());
                }
                Shape::Circle(c) if c.fill == p.acc && rect.contains(c.center) => dots += 1,
                _ => {}
            }
        }
        (lines, dots)
    }

    #[test]
    fn sparklines_draw_nothing_without_samples_a_dot_for_one_and_lines_for_more() {
        let now = Utc::now();
        // (caso, histórico, (linhas com N pontos, pontos))
        type Case = (&'static str, VecDeque<Sample>, (Vec<usize>, usize));
        let cases: [Case; 5] = [
            ("no samples", VecDeque::new(), (vec![], 0)),
            // Sem o ai-memory no meio: dois pontos, nunca uma linha por 0.
            (
                "missing in the middle",
                VecDeque::from([
                    sample(now, 20, Some((10., 400)), Some((15., 700))),
                    sample(now, 10, None, Some((15., 700))),
                    sample(now, 0, Some((12., 410)), Some((15., 700))),
                ]),
                (vec![], 2),
            ),
            (
                "docker only",
                VecDeque::from([
                    sample(now, 10, None, Some((5., 100))),
                    sample(now, 0, None, Some((6., 100))),
                ]),
                (vec![], 0),
            ),
            (
                "one sample",
                VecDeque::from([sample(now, 0, Some((10., 400)), None)]),
                (vec![], 1),
            ),
            // Isolada há 4 min (ponto) e as três seguidas (linha).
            ("gap", history(now), (vec![3], 1)),
        ];
        for (theme, p) in THEMES {
            for (case, h, want) in &cases {
                let mut page = Page::new(theme, WIDE);
                page.history = h.clone();
                let out = page.show();
                for card in ["ai-memory · CPU", "ai-memory · Memória"] {
                    assert_eq!(&spark(&out, card, p), want, "{theme:?} {case} {card}");
                }
            }
        }
    }

    #[test]
    fn sparklines_span_their_samples_while_the_chart_keeps_the_window() {
        for (theme, p) in THEMES {
            let mut page = Page::new(theme, WIDE);
            let now = page.now;
            // Dez segundos de histórico numa janela de 5 min.
            page.history = VecDeque::from([
                sample(now, 10, Some((10., 400)), Some((15., 700))),
                sample(now, 5, Some((20., 300)), Some((25., 700))),
                sample(now, 0, Some((10., 400)), Some((15., 700))),
            ]);
            let out = page.show();
            for card in ["ai-memory · CPU", "ai-memory · Memória"] {
                let plot = spark_rect(&out, card).shrink2(vec2(1., 2.));
                let xs: Vec<Vec<f32>> = shapes(&out)
                    .into_iter()
                    .filter_map(|s| match s {
                        Shape::Path(path)
                            if path.stroke.color == ColorMode::Solid(p.acc)
                                && path.points.iter().all(|&pt| plot.contains(pt)) =>
                        {
                            Some(path.points.iter().map(|pt| pt.x).collect())
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    xs,
                    vec![vec![plot.left(), plot.center().x, plot.right()]],
                    "{theme:?} {card}: from the first sample to the last"
                );
            }
            let chart = plot(&out);
            let line = lines_in(&out, chart, p.acc, 1.75);
            let at = |ago: f32| chart.left() + chart.width() * (300. - ago) / 300.;
            assert!(
                line.len() == 1
                    && line[0].len() == 3
                    && line[0]
                        .iter()
                        .zip([at(10.), at(5.), at(0.)])
                        .all(|(pt, x)| (pt.x - x).abs() < 0.01),
                "{theme:?}: the chart keeps the 5 min axis: {line:?}"
            );
            // Uma amostra: o ponto na borda direita.
            page.history = VecDeque::from([sample(now, 0, Some((10., 400)), None)]);
            let out = page.show();
            let plot = spark_rect(&out, "ai-memory · CPU").shrink2(vec2(1., 2.));
            let dots: Vec<Pos2> = shapes(&out)
                .into_iter()
                .filter_map(|s| match s {
                    Shape::Circle(c) if c.fill == p.acc && plot.contains(c.center) => {
                        Some(c.center)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                dots,
                vec![pos2(plot.right(), plot.bottom() - plot.height())]
            );
        }
    }

    #[test]
    fn chart_preserves_isolated_samples_and_scales_gap_with_resource_interval() {
        let base = Utc::now();
        let u = Some(Usage {
            cpu: 50.0,
            memory: 128 << 20,
            ..Default::default()
        });
        // 1) An isolated valid sample followed by a failed (None) sample must still render as a dot.
        let history = VecDeque::from([
            Sample {
                at: base - chrono::Duration::seconds(10),
                ai: u.clone(),
                ai_in_docker: false,
                docker: None,
            },
            Sample {
                at: base - chrono::Duration::seconds(8),
                ai: None,
                ai_in_docker: false,
                docker: None,
            },
        ]);
        let plot = egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(200., 100.));
        let ai = |s: &Sample| s.ai.as_ref().map(|u| u.cpu);
        let (lines, dots) = segments(&history, 5, 2, 100.0, plot, ai);
        assert!(lines.is_empty());
        assert_eq!(
            dots.len(),
            1,
            "isolated sample before a None gap must not be erased"
        );

        // 2) With resource_seconds = 60, samples 66s apart must connect as a line, not blank out.
        let history_60s = VecDeque::from([
            Sample {
                at: base - chrono::Duration::seconds(132),
                ai: u.clone(),
                ai_in_docker: false,
                docker: None,
            },
            Sample {
                at: base - chrono::Duration::seconds(66),
                ai: u.clone(),
                ai_in_docker: false,
                docker: None,
            },
            Sample {
                at: base,
                ai: u,
                ai_in_docker: false,
                docker: None,
            },
        ]);
        let (lines, _) = segments(&history_60s, 5, 60, 100.0, plot, ai);
        assert_eq!(
            lines.len(),
            1,
            "60s interval samples separated by 66s must form a connected line"
        );
        assert_eq!(lines[0].len(), 3);
        // 3) With 2 s collections the same 66 s apart break (limit 65 s), and
        // time going backwards breaks too.
        let (lines, dots) = segments(&history_60s, 5, 2, 100.0, plot, ai);
        assert_eq!((lines.len(), dots.len()), (0, 3));
        let mut back = history_60s.clone();
        back.swap(1, 2);
        let (lines, dots) = segments(&back, 5, 60, 100.0, plot, ai);
        assert_eq!((lines.len(), dots.len()), (1, 1), "back in time breaks");
    }

    /// O retângulo do gráfico (nó `Image`).
    fn plot(out: &egui::FullOutput) -> Rect {
        let found: Vec<Rect> = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == Role::Image)
            .map(|(_, n)| rect_of(n))
            .collect();
        assert_eq!(found.len(), 1, "one chart");
        found[0]
    }
    fn meshes(out: &egui::FullOutput, color: Color32) -> Vec<Vec<Pos2>> {
        shapes(out)
            .into_iter()
            .filter_map(|s| match s {
                Shape::Mesh(m) if m.vertices.iter().all(|v| v.color == color) => {
                    Some(m.vertices.iter().map(|v| v.pos).collect())
                }
                _ => None,
            })
            .collect()
    }
    fn lines_in(out: &egui::FullOutput, rect: Rect, color: Color32, width: f32) -> Vec<Vec<Pos2>> {
        shapes(out)
            .into_iter()
            .filter_map(|s| match s {
                Shape::Path(path)
                    if path.stroke.color == ColorMode::Solid(color)
                        && path.stroke.width == width
                        && path.points.iter().all(|&pt| rect.expand(1.).contains(pt)) =>
                {
                    Some(path.points)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn chart_stacks_others_over_ai_memory_and_never_below_zero() {
        for (theme, p) in THEMES {
            let mut page = Page::new(theme, WIDE);
            let now = page.now;
            // ai < Docker; ai acima do Docker (outros = 0); sem o ai-memory
            // (outros = o total).
            page.history = VecDeque::from([
                sample(now, 20, Some((10., 1)), Some((15., 1))),
                sample(now, 10, Some((12., 1)), Some((11., 1))),
                sample(now, 0, None, Some((14., 1))),
            ]);
            let out = page.show();
            let plot = plot(&out);
            // Pico 15 → topo 20%.
            let (x, y) = (
                |ago: f32| plot.left() + plot.width() * (300. - ago) / 300.,
                |v: f32| plot.bottom() - plot.height() * v / 20.,
            );
            let near = |got: &[Pos2], want: &[(f32, f32)]| {
                got.len() == want.len()
                    && got
                        .iter()
                        .zip(want)
                        .all(|(g, w)| (g.x - w.0).abs() < 0.01 && (g.y - w.1).abs() < 0.01)
            };
            let others = meshes(&out, widgets::tone(p.series2, 0.22));
            let want_others = [
                (x(20.), y(15.)),
                (x(10.), y(12.)),
                (x(10.), y(12.)),
                (x(20.), y(10.)),
                (x(10.), y(12.)),
                (x(0.), y(14.)),
                (x(0.), y(0.)),
                (x(10.), y(12.)),
            ];
            assert!(
                others.len() == 1 && near(&others[0], &want_others),
                "{theme:?}: others band {others:?}"
            );
            let ai = meshes(&out, widgets::tone(p.acc, 0.28));
            let want_ai = [
                (x(20.), y(10.)),
                (x(10.), y(12.)),
                (x(10.), y(0.)),
                (x(20.), y(0.)),
            ];
            assert!(
                ai.len() == 1 && near(&ai[0], &want_ai),
                "{theme:?}: ai band {ai:?}"
            );
            let total = lines_in(&out, plot, p.series2, 1.5);
            assert!(
                total.len() == 1
                    && near(
                        &total[0],
                        &[(x(20.), y(15.)), (x(10.), y(12.)), (x(0.), y(14.))]
                    ),
                "{theme:?}: total line {total:?}"
            );
            let line = lines_in(&out, plot, p.acc, 1.75);
            assert!(
                line.len() == 1 && near(&line[0], &[(x(20.), y(10.)), (x(10.), y(12.))]),
                "{theme:?}: ai line {line:?}"
            );
            // O tooltip de cada amostra: outros nunca negativos.
            let time = |ago: i64| {
                (now - chrono::Duration::seconds(ago))
                    .with_timezone(&Local)
                    .format("%H:%M:%S")
                    .to_string()
            };
            for (ago, want) in [
                (20, "ai-memory 10,0% · Outros 5,0% · Total Docker 15,0%"),
                (10, "ai-memory 12,0% · Outros 0,0% · Total Docker 11,0%"),
                (0, "ai-memory — · Outros 14,0% · Total Docker 14,0%"),
            ] {
                let out = page.hover(pos2(x(ago as f32), plot.center().y));
                node(&out, Role::Label, &format!("{} · {want}", time(ago)));
            }
        }
    }

    #[test]
    fn chart_never_subtracts_a_native_ai_memory_from_the_docker_total() {
        for (theme, p) in THEMES {
            let mut page = Page::new(theme, WIDE);
            let now = page.now;
            // As amostras do teste acima, com o ai-memory nativo: fora do
            // total do Docker, os outros são o total inteiro, por cima dele.
            let native = |ago, ai, docker| Sample {
                ai_in_docker: false,
                ..sample(now, ago, Some(ai), Some(docker))
            };
            page.history = VecDeque::from([
                native(10, (10., 1), (15., 1)),
                native(0, (12., 1), (11., 1)),
            ]);
            let out = page.show();
            let plot = plot(&out);
            // Topo da pilha 12 + 11 = 23 → 26,45 → eixo até 50%.
            let (x, y) = (
                |ago: f32| plot.left() + plot.width() * (300. - ago) / 300.,
                |v: f32| plot.bottom() - plot.height() * v / 50.,
            );
            let near = |got: &[Pos2], want: &[(f32, f32)]| {
                got.len() == want.len()
                    && got
                        .iter()
                        .zip(want)
                        .all(|(g, w)| (g.x - w.0).abs() < 0.01 && (g.y - w.1).abs() < 0.01)
            };
            let total = lines_in(&out, plot, p.series2, 1.5);
            assert!(
                total.len() == 1 && near(&total[0], &[(x(10.), y(25.)), (x(0.), y(23.))]),
                "{theme:?}: others stacked whole over the native ai-memory {total:?}"
            );
            let others = meshes(&out, widgets::tone(p.series2, 0.22));
            assert!(
                others.len() == 1
                    && near(
                        &others[0],
                        &[
                            (x(10.), y(25.)),
                            (x(0.), y(23.)),
                            (x(0.), y(12.)),
                            (x(10.), y(10.)),
                        ]
                    ),
                "{theme:?}: others band {others:?}"
            );
            let time = |ago: i64| {
                (now - chrono::Duration::seconds(ago))
                    .with_timezone(&Local)
                    .format("%H:%M:%S")
                    .to_string()
            };
            for (ago, want) in [
                (10, "ai-memory 10,0% · Outros 15,0% · Total Docker 15,0%"),
                (0, "ai-memory 12,0% · Outros 11,0% · Total Docker 11,0%"),
            ] {
                let out = page.hover(pos2(x(ago as f32), plot.center().y));
                node(&out, Role::Label, &format!("{} · {want}", time(ago)));
            }
        }
    }

    #[test]
    fn segments_switch_metric_and_window_and_the_axes_follow() {
        let mut page = Page::new(egui::Theme::Light, WIDE);
        // (métrica, janela, rótulos do eixo y de cima para baixo, eixo x)
        for (metric, minutes, y, x) in [
            (
                "CPU",
                "5 min",
                ["100%", "50%", "0%"],
                ["−5 min", "−2,5 min", "agora"],
            ),
            (
                "CPU",
                "1 min",
                ["20%", "10%", "0%"],
                ["−1 min", "−0,5 min", "agora"],
            ),
            (
                "Memória",
                "1 min",
                ["1 GiB", "0,5 GiB", "0 GiB"],
                ["−1 min", "−0,5 min", "agora"],
            ),
            (
                "Memória",
                "10 min",
                ["2 GiB", "1 GiB", "0 GiB"],
                ["−10 min", "−5 min", "agora"],
            ),
            (
                "CPU",
                "10 min",
                ["100%", "50%", "0%"],
                ["−10 min", "−5 min", "agora"],
            ),
        ] {
            for segment in [metric, minutes] {
                let out = page.show();
                if node(&out, Role::Button, segment).toggled()
                    != Some(egui::accesskit::Toggled::True)
                {
                    page.click_on(Role::Button, segment);
                }
            }
            let out = page.show();
            let plot = plot(&out);
            let window = match minutes {
                "1 min" => "no último minuto".to_owned(),
                m => format!("nos últimos {} minutos", &m[..m.len() - 4]),
            };
            node(
                &out,
                Role::Image,
                &format!("{metric} {window}: ai-memory e outros contêineres empilhados"),
            );
            let card = pane(&out, "Recursos");
            let painted = |filter: &dyn Fn(Rect) -> bool| {
                let mut found: Vec<(f32, f32, String)> = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        Shape::Text(t) if card.contains(t.pos) => {
                            let r = t.visual_bounding_rect();
                            filter(r)
                                .then(|| (r.center().y, r.center().x, t.galley.text().to_owned()))
                        }
                        _ => None,
                    })
                    .collect();
                found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
                found.into_iter().map(|(.., t)| t).collect::<Vec<_>>()
            };
            assert_eq!(
                painted(&|r| r.right() <= plot.left() && r.top() >= plot.top() - 10.),
                y,
                "{metric} {minutes}: y axis"
            );
            assert_eq!(
                painted(&|r| r.top() >= plot.bottom()),
                x,
                "{metric} {minutes}: x axis"
            );
        }
    }

    #[test]
    fn scale_picks_round_tops_and_labels_in_the_unit_of_the_top() {
        for (peak, want) in [
            (0., 10.),
            (8.69, 10.),
            (9., 20.),
            (15.5, 20.),
            (20., 25.),
            (30., 50.),
            (100., 200.),
            (250., 500.),
            (1000., 2000.),
        ] {
            assert_eq!(scale(Metric::Cpu, peak), want, "CPU {peak}");
        }
        let m = M as f64;
        for (peak, want) in [(0., 64.), (60., 128.), (421., 512.), (900., 2048.)] {
            assert_eq!(
                scale(Metric::Memory, peak * m),
                want * m,
                "memory {peak} MiB"
            );
        }
        for (metric, v, max, want) in [
            (Metric::Cpu, 12.5, 25., "12,5%"),
            (Metric::Cpu, 0., 10., "0%"),
            (Metric::Memory, 32. * m, 64. * m, "32 MiB"),
            (Metric::Memory, 512. * m, 1024. * m, "0,5 GiB"),
            (Metric::Memory, 0., 2048. * m, "0 GiB"),
        ] {
            assert_eq!(axis_label(metric, v, max), want);
        }
    }

    #[test]
    fn the_tooltip_follows_the_nearest_sample_and_survives_a_new_collection() {
        // (tema, tamanho, a caixa da amostra de 4 min atrás à direita da
        // linha?): em 594 não cabe à esquerda.
        for (theme, size, right) in [
            (egui::Theme::Dark, WIDE, false),
            (egui::Theme::Light, WIDE, false),
            (egui::Theme::Dark, NARROW, true),
            (egui::Theme::Light, NARROW, true),
        ] {
            let mut page = Page::new(theme, size);
            let now = page.now;
            let out = page.show();
            let plot = plot(&out);
            let time = |at: DateTime<Utc>| at.with_timezone(&Local).format("%H:%M:%S").to_string();
            let tip = |out: &egui::FullOutput| {
                out.platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .filter(|(_, n)| {
                        n.role() == Role::Label
                            && n.value().is_some_and(|v| v.contains(" · Total Docker "))
                    })
                    .map(|(_, n)| (n.value().unwrap().to_owned(), rect_of(n)))
                    .collect::<Vec<_>>()
            };
            assert!(
                tip(&out).is_empty(),
                "{theme:?}: no tooltip without the pointer"
            );
            // Na borda direita: a última amostra, a caixa à esquerda da linha.
            let out = page.hover(pos2(plot.right() - 1., plot.center().y));
            let found = tip(&out);
            assert_eq!(
                found.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
                [format!(
                    "{} · ai-memory 10,4% · Outros 5,1% · Total Docker 15,5%",
                    time(now)
                )],
                "{theme:?}"
            );
            assert!(
                found[0].1.right() <= plot.right() - 10. + 0.5 && plot.contains_rect(found[0].1)
            );
            // Na borda esquerda: a isolada de 4 min atrás, a caixa à direita.
            let out = page.hover(pos2(plot.left() + 1., plot.center().y));
            let found = tip(&out);
            assert_eq!(
                found.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
                [format!(
                    "{} · ai-memory 49,4% · Outros 10,6% · Total Docker 60,0%",
                    time(now - chrono::Duration::seconds(240))
                )]
            );
            let line = plot.left() + plot.width() * 60. / 300.;
            let box_ = found[0].1;
            assert!(
                plot.contains_rect(box_),
                "{theme:?} {size:?}: {box_:?} in {plot:?}"
            );
            if right {
                assert!(
                    (box_.left() - (line + 10.)).abs() < 0.5,
                    "{size:?}: {box_:?}"
                );
            } else {
                assert!(
                    (box_.right() - (line - 10.)).abs() < 0.5,
                    "{size:?}: {box_:?}"
                );
            }
            // Coleta nova com o ponteiro parado: some a amostra de baixo dele.
            page.history = VecDeque::new();
            let out = page.show();
            assert!(tip(&out).is_empty());
            node(&out, Role::Label, "Aguardando amostras…");
            let later = now + chrono::Duration::seconds(2);
            page.history = VecDeque::from([sample(later, 0, Some((1., 10)), Some((2., 20)))]);
            let out = page.show();
            assert_eq!(
                tip(&out).into_iter().map(|(t, _)| t).collect::<Vec<_>>(),
                [format!(
                    "{} · ai-memory 1,0% · Outros 1,0% · Total Docker 2,0%",
                    time(later)
                )]
            );
            assert!(!has(&out, Role::Label, "Aguardando amostras…"));
        }
    }

    #[test]
    fn controls_share_the_title_row_only_when_they_fit_and_keep_their_order() {
        for (size, beside) in [(WIDE, true), (NARROW, false)] {
            let mut page = Page::new(egui::Theme::Dark, size);
            let out = page.show();
            let title = rect_of(&node(&out, Role::Label, "Recursos"));
            let at = |text| rect_of(&node(&out, Role::Button, text));
            let (cpu, memory) = (at("CPU"), at("Memória"));
            let (one, five, ten) = (at("1 min"), at("5 min"), at("10 min"));
            assert!(
                cpu.right() <= memory.left()
                    && one.right() <= five.left()
                    && five.right() <= ten.left(),
                "{size:?}: left to right, as written"
            );
            assert!(memory.right() < one.left(), "{size:?}: metric, then window");
            assert!(
                (cpu.top() - one.top()).abs() < 0.5,
                "{size:?}: one row, {cpu:?} / {one:?}"
            );
            assert_eq!(
                (cpu.center().y - title.center().y).abs() < 2.,
                beside,
                "{size:?}: {cpu:?} / {title:?}"
            );
            if beside {
                let card = pane(&out, "Recursos");
                assert!(
                    (ten.right() + 2. + 1. - (card.right() - 1. - 16.)).abs() < 0.5,
                    "{size:?}: {ten:?} at the right of {card:?}"
                );
            } else {
                assert!(cpu.top() > title.bottom(), "{size:?}");
            }
            for (name, rect) in [("CPU", cpu), ("10 min", ten)] {
                assert!(
                    pane(&out, "Recursos").contains_rect(rect),
                    "{size:?}: {name}"
                );
            }
        }
    }

    #[test]
    fn the_health_summary_goes_below_only_when_it_lacks_320() {
        for (size, beside) in [(WIDE, true), (NARROW, true), ([460., 1400.], false)] {
            let mut page = Page::new(egui::Theme::Light, size);
            let out = page.show();
            let strip = pane(&out, "Saúde");
            let title = rect_of(&node(&out, Role::Label, "Tudo funcionando"));
            let summary = inside(&out, Role::Label, strip)
                .into_iter()
                .find(|t| t.starts_with("ai-memory 3.3.1"))
                .unwrap();
            let summary = rect_of(&node(&out, Role::Label, &summary));
            let link = rect_of(&node(&out, Role::Link, "Ver saúde →"));
            assert_eq!(summary.top() > title.bottom(), !beside, "{size:?}");
            assert!(
                summary.right() <= link.left() || summary.top() > link.bottom(),
                "{size:?}"
            );
            assert!(
                strip.contains_rect(summary) && strip.contains_rect(link),
                "{size:?}"
            );
            if beside {
                assert!(summary.left() > title.right(), "{size:?}");
            }
        }
    }

    #[test]
    fn card_lines_explain_what_they_count_on_hover() {
        // (cartão, linha de baixo, tooltip; o que corta mostra a linha inteira)
        let mut long = api(Utc::now());
        long.projects = vec![
            Project {
                open_session_count: Some(123_456_789_012),
                ..Default::default()
            };
            1
        ];
        for (size, a, card, sub, tip) in [
            (
                WIDE,
                api(Utc::now()),
                "Contêineres",
                "demais contêineres: 3,1% · 312,0 MiB",
                OTHERS_HINT.to_owned(),
            ),
            (
                WIDE,
                api(Utc::now()),
                "Continuidade",
                "11 sessões abertas · 3 propostas",
                CONTINUITY_HINT.to_owned(),
            ),
            (
                [480., 1400.],
                long,
                "Continuidade",
                "123.456.789.012 sessões abertas · 3 propostas",
                format!("123.456.789.012 sessões abertas · 3 propostas\n\n{CONTINUITY_HINT}"),
            ),
        ] {
            let mut page = Page::new(egui::Theme::Dark, size);
            page.api = a;
            let out = page.show();
            let line = rect_of(&node(&out, Role::Label, sub));
            assert!(pane(&out, card).contains_rect(line), "{card}");
            assert!(!has(&out, Role::Label, &tip), "{card}: only on hover");
            let out = page.rest(line.center());
            node(&out, Role::Label, &tip);
        }
    }

    #[test]
    fn running_containers_are_listed_by_cpu_and_open_in_docker() {
        let rows = |out: &egui::FullOutput| {
            let mut found: Vec<(f32, String, Rect)> = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, n)| {
                    n.role() == Role::Button && n.label().is_some_and(|l| l.contains(" · CPU "))
                })
                .map(|(_, n)| (rect_of(n).top(), n.label().unwrap().to_owned(), rect_of(n)))
                .collect();
            found.sort_by(|a, b| a.0.total_cmp(&b.0));
            found
                .into_iter()
                .map(|(_, l, r)| (l, r))
                .collect::<Vec<_>>()
        };
        for (theme, _) in THEMES {
            let mut page = Page::new(theme, NARROW);
            let mut no_stats = running("sem-stats", 0., 0, 0);
            no_stats.usage = None;
            page.resources.containers.push(no_stats);
            let out = page.show();
            let table = pane(&out, "Contêineres em execução");
            node(&out, Role::Label, "Contêineres em execução 4 de 9");
            let found = rows(&out);
            assert_eq!(
                found.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
                [
                    "ai-memory · CPU 12,4% · Memória 421,0 MiB · Tarefas 46",
                    "postgres-dev · CPU 2,7% · Memória 218,0 MiB · Tarefas 12",
                    "redis-cache · CPU 0,4% · Memória 94,0 MiB · Tarefas 6",
                    "sem-stats · CPU — · Memória — · Tarefas —",
                ],
                "{theme:?}: running only, by CPU"
            );
            assert!(
                found
                    .iter()
                    .all(|(_, r)| r.height() == ROW && table.contains_rect(*r))
            );
            assert!(found.windows(2).all(|w| w[0].1.bottom() == w[1].1.top()));
            let header = inside(&out, Role::Label, table);
            assert_eq!(
                header[1..5],
                ["Nome", "CPU", "Memória", "Tarefas"],
                "{theme:?}"
            );
            // "Tarefas" explica o que conta.
            let tasks = rect_of(&node(&out, Role::Label, "Tarefas"));
            assert!(!has(&out, Role::Label, docker_ui::TASKS_HINT));
            let out = page.rest(tasks.center());
            node(&out, Role::Label, docker_ui::TASKS_HINT);
            let postgres = found[1].1;
            page.click(postgres.center());
            page.click_on(Role::Link, "Ver todos →");
            assert_eq!(
                page.go,
                [
                    Go::Docker(Some("postgres-dev-8f3c21a9d0e4".into())),
                    Go::Docker(None)
                ],
                "{theme:?}"
            );
        }
        let now = Utc::now();
        // (caso, recursos, contagem do título, mensagem)
        for (case, r, count, note) in [
            (
                "no collection",
                ResourceSnapshot::default(),
                "—",
                "Aguardando a primeira coleta do Docker.",
            ),
            (
                "docker down",
                ResourceSnapshot {
                    at: Some(now),
                    docker_error: Some("x".into()),
                    ..Default::default()
                },
                "—",
                "Docker indisponível. Veja o aviso acima.",
            ),
            (
                "none running",
                ResourceSnapshot {
                    at: Some(now),
                    containers: vec![stopped("a"), stopped("b")],
                    ..Default::default()
                },
                "0 de 2",
                "Nenhum contêiner em execução.",
            ),
        ] {
            let mut page = Page::new(egui::Theme::Dark, WIDE);
            page.resources = r;
            let out = page.show();
            let table = pane(&out, "Contêineres em execução");
            assert_eq!(
                inside(&out, Role::Label, table),
                [
                    format!("Contêineres em execução {count}"),
                    "Nome".into(),
                    "CPU".into(),
                    "Memória".into(),
                    "Tarefas".into(),
                    note.into(),
                ],
                "{case}"
            );
            assert!(rows(&out).is_empty(), "{case}");
        }
    }

    #[test]
    fn recent_handoffs_show_the_four_newest_with_restricted_missing_and_messages() {
        let now = Utc::now();
        for (theme, _) in THEMES {
            let mut page = Page::new(theme, WIDE);
            let out = page.show();
            let card = pane(&out, "Handoffs recentes");
            let restricted = sessions_ui::age(&page.api.handoffs[3].at, now)
                .map(|a| a.long().unwrap_or_else(|| a.short()))
                .unwrap();
            assert_eq!(
                inside(&out, Role::Label, card),
                [
                    "Handoffs recentes 75 abertos".to_owned(),
                    "acme / api-pagamentos · há 12 min · Critério de desempate".into(),
                    "acme / portal-web · há 38 min · Migração concluída".into(),
                    "pessoal / notas · há 2 h · Sem resumo".into(),
                    format!("acme / infra · {restricted} · Resumo restrito"),
                ],
                "{theme:?}"
            );
            assert!(
                !inside(&out, Role::Label, card)
                    .iter()
                    .any(|l| l.contains("quinto")
                        || l.contains("cross-session")
                        || l.contains("segredo")),
                "{theme:?}: only four, no markup, nothing restricted"
            );
            page.click_on(Role::Link, "Abrir →");
            assert_eq!(page.go, [Go::Sessions]);
        }
        // (caso, API, título, mensagem)
        let listed = |total: Option<usize>, n: usize| ApiSnapshot {
            handoff_total: total,
            handoffs: api(now).handoffs.into_iter().take(n).collect(),
            ..api(now)
        };
        for (case, a, head, note) in [
            (
                "none",
                listed(Some(0), 0),
                "Handoffs recentes 0 abertos",
                Some("Nenhum handoff aberto."),
            ),
            (
                "one",
                listed(Some(1), 1),
                "Handoffs recentes 1 aberto",
                None,
            ),
            (
                "list only",
                listed(None, 3),
                "Handoffs recentes 3+ abertos",
                None,
            ),
            (
                "before the 1st collection",
                ApiSnapshot::default(),
                "Handoffs recentes —",
                Some("Aguardando a consulta ao ai-memory."),
            ),
        ] {
            let mut page = Page::new(egui::Theme::Dark, WIDE);
            let rows = a.handoffs.len().min(RECENT);
            page.api = a;
            let out = page.show();
            let found = inside(&out, Role::Label, pane(&out, "Handoffs recentes"));
            assert_eq!(found[0], head, "{case}");
            assert_eq!(
                found.len(),
                1 + rows + usize::from(note.is_some()),
                "{case}: {found:?}"
            );
            if let Some(note) = note {
                assert_eq!(found[1], note, "{case}");
            }
        }
    }
}
