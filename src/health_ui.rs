//! Página Saúde (Health.dc.html): linha de coleta com "Atualizar agora",
//! estado geral, grade de cartões, recolhíveis e legenda. Dados do servidor
//! são exibidos como dados. Campo ausente ou recusado aparece como "—",
//! nunca como 0.
use crate::{
    console::{array, count_grid, number, row_key, show_values, text},
    health::{self, Snapshot},
    model::{bytes, decimal, metric_value},
    monitor::Shared,
    theme,
    widgets::{self, Fact, Tone, Type},
};
use chrono::{DateTime, Local, Utc};
use eframe::egui::{self, Color32, RichText, Ui, pos2, vec2};
use serde_json::Value;

/// Títulos dos cartões da grade, na ordem do artboard.
pub(crate) const CARDS: [&str; 6] = [
    "Captura",
    "Consolidação",
    "Fila de escrita",
    "Providers",
    "Armazenamento",
    "Índices",
];
const CAPTION: &str = "Campo ausente = desconhecido. Sem embeddings pode ser normal quando o recurso não está configurado.";

/// Estado geral, na mesma ordem do resumo da barra de menus
/// (`Snapshot::menu_summary`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Overall {
    Waiting,
    Stale,
    Unavailable(String),
    Alerts(usize),
    Healthy,
}
pub(crate) fn overall(s: &Snapshot, now: DateTime<Utc>) -> Overall {
    if s.at.is_none() {
        return Overall::Waiting;
    }
    if !s.fresh(now) {
        return Overall::Stale;
    }
    if let Some(error) = &s.error {
        return Overall::Unavailable(error.to_string());
    }
    match s.alerts().len() {
        0 => Overall::Healthy,
        n => Overall::Alerts(n),
    }
}
impl Overall {
    pub(crate) fn title(&self) -> &'static str {
        match self {
            Overall::Waiting => "Aguardando",
            Overall::Stale => "Leitura atrasada",
            Overall::Unavailable(_) => "Indisponível",
            Overall::Alerts(_) => "Com alertas",
            Overall::Healthy => "Saudável",
        }
    }
    pub(crate) fn subtitle(&self) -> String {
        let hint = crate::platform::HEALTH_ALERT_HINT;
        match self {
            Overall::Waiting => "A primeira coleta de saúde ainda não terminou.".into(),
            Overall::Stale => format!(
                "A última coleta tem mais de {} s; os números abaixo podem estar desatualizados.",
                health::INTERVAL * 3
            ),
            Overall::Unavailable(error) => error.clone(),
            Overall::Alerts(1) => {
                format!("1 alerta nas verificações disponíveis, listado abaixo. {hint}")
            }
            Overall::Alerts(n) => {
                format!("{n} alertas nas verificações disponíveis, listados abaixo. {hint}")
            }
            Overall::Healthy => format!("Sem alertas nas verificações disponíveis. {hint}"),
        }
    }
}

/// A página. `refreshing` = há um "Atualizar agora" ainda não atendido;
/// com a tela pausada o botão fica desabilitado (o resultado não apareceria).
pub(crate) fn show(
    ui: &mut Ui,
    shared: &Shared,
    s: &Snapshot,
    refreshing: bool,
    paused: bool,
    now: DateTime<Utc>,
) {
    let p = theme::palette(ui.visuals());
    let every = format!("a cada {} s, mesmo com a janela fechada", health::INTERVAL);
    widgets::toolbar(
        ui,
        |ui| {
            let line = match s.at {
                Some(at) => format!(
                    "Coletado {} · {every}",
                    crate::app::ago((now - at).num_seconds())
                ),
                None => format!("Aguardando a primeira coleta · {every}"),
            };
            let response = widgets::caption(ui, &line);
            if let Some(at) = s.at {
                response.on_hover_text(format!(
                    "Última tentativa: {}",
                    at.with_timezone(&Local).format("%H:%M:%S")
                ));
            }
        },
        |ui| {
            let label = if refreshing {
                "Atualizando…"
            } else {
                "Atualizar agora"
            };
            let response =
                ui.add_enabled(!refreshing && !paused, widgets::Button::secondary(label));
            if response.clicked() {
                shared.refresh_health();
            }
            if paused {
                response.on_disabled_hover_text("Retome a atualização da tela para atualizar.");
            }
        },
    );
    let state = overall(s, now);
    status_card(ui, &state, s.identity.as_ref());
    if s.status.is_some() {
        for alert in s.alerts() {
            widgets::strip(ui, Tone::Warn, &format!("{alert}."), "", None);
        }
    }
    for error in &s.auxiliary_errors {
        widgets::strip(ui, Tone::Warn, "Coleta parcial.", error, None);
    }
    let v = s.status.as_ref().unwrap_or(&Value::Null);
    widgets::card_grid(ui, &CARDS, |ui, i| match i {
        0 => widgets::facts(
            ui,
            &[
                Fact::new("Páginas atuais", number(&v["counts"]["pages_latest"])),
                Fact::new("Sessões", number(&v["counts"]["sessions"])),
                Fact::new("Eventos", number(&v["counts"]["observations"])),
            ],
        ),
        1 => consolidation(ui, &v["consolidation"]),
        2 => write_queue(ui, &v["write_queue"]),
        3 => providers(ui, &v["providers"]),
        4 => {
            let free = v["storage"]["data_dir_free_bytes"].as_u64();
            widgets::facts(
                ui,
                &[
                    Fact::new("Banco de dados", size(&v["storage"]["database_bytes"])),
                    Fact::new(
                        "Potencial recuperável",
                        size(&v["storage"]["reclaimable_bytes"]),
                    ),
                    Fact::new(
                        "Disco livre no servidor",
                        free.map(bytes).unwrap_or("—".into()),
                    )
                    .color(free.filter(|&n| n < 512 * 1024 * 1024).map(|_| p.warn)),
                ],
            );
        }
        _ => indices(ui, &v["derived"], p),
    });
    widgets::card(ui).inner_margin(0).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.;
        let series = match s.metrics.len() {
            1 => "1 série".to_owned(),
            n => format!("{} séries", crate::model::number(n)),
        };
        widgets::disclosure_row(
            ui,
            "metrics",
            "Métricas acumuladas desde o início do servidor",
            &series,
            |ui| {
                ui.reset_style();
                for m in &s.metrics {
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(&m.series);
                        ui.label(metric_value(m.value));
                    });
                }
                if s.metrics.is_empty() {
                    ui.label("Nenhuma métrica disponível.");
                }
            },
        );
        let (line, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 1.), egui::Sense::hover());
        ui.painter().rect_filled(line, 0., p.line);
        widgets::disclosure_row(
            ui,
            "raw",
            "Dados brutos informados pelo servidor",
            "JSON",
            |ui| {
                ui.reset_style();
                for (title, value) in [
                    ("Status (/admin/status)", &s.status),
                    ("Recursos (/api/v1/docs/features/status)", &s.features),
                ] {
                    ui.label(RichText::new(title).font(Type::Section.font()));
                    match value {
                        Some(value) => {
                            ui.push_id(title, |ui| show_values(ui, value, 0));
                        }
                        None => {
                            widgets::caption(ui, "Não informado.");
                        }
                    }
                }
            },
        );
    });
    widgets::caption(ui, CAPTION);
}

fn size(v: &Value) -> String {
    v.as_u64().map(bytes).unwrap_or("—".into())
}

/// Ícone de estado num círculo de 40 px, título, subtítulo e a identidade.
fn status_card(ui: &mut Ui, state: &Overall, identity: Option<&health::Identity>) {
    let p = theme::palette(ui.visuals());
    let (fill, color) = match state {
        Overall::Healthy => (p.sel, p.acc),
        Overall::Alerts(_) | Overall::Stale => (widgets::tone(p.warn, 0.12), p.warn),
        Overall::Unavailable(_) => (widgets::tone(p.bad, 0.08), p.bad),
        Overall::Waiting => (p.surf2, p.text2),
    };
    let dash = |v: &str| {
        if v.is_empty() {
            "—".to_owned()
        } else {
            v.to_owned()
        }
    };
    let rows = match identity {
        Some(i) => [
            ("Versão", dash(&i.version)),
            (
                "Acesso",
                match i.level.as_str() {
                    "anonymous" => "Anônimo".to_owned(),
                    level => dash(level),
                },
            ),
            (
                "Distingue operadores",
                if i.distinguishes_operators {
                    "Sim"
                } else {
                    "Não"
                }
                .to_owned(),
            ),
        ],
        None => [
            ("Versão", "—".to_owned()),
            ("Acesso", "—".to_owned()),
            ("Distingue operadores", "—".to_owned()),
        ],
    };
    widgets::card(ui)
        .corner_radius(12.)
        .inner_margin(egui::Margin::symmetric(20, 18))
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            // A identidade vai à direita se o texto ainda tiver 220 px.
            const IDENTITY: f32 = 210.;
            let beside = width - 56. - IDENTITY - 16. >= 220.;
            let text_width = if beside {
                width - 56. - IDENTITY - 16.
            } else {
                width - 56.
            };
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.;
                let (circle, _) = ui.allocate_exact_size(vec2(40., 40.), egui::Sense::hover());
                ui.painter().circle_filled(circle.center(), 20., fill);
                let icon = egui::Rect::from_center_size(circle.center(), vec2(22., 22.));
                state_icon(ui.painter(), icon, state, color);
                ui.allocate_ui(vec2(text_width, 0.), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.;
                    ui.set_width(text_width);
                    ui.label(RichText::new(state.title()).font(Type::Detail.font()));
                    ui.add(
                        egui::Label::new(RichText::new(state.subtitle()).color(p.text2).size(13.))
                            .wrap(),
                    );
                });
                if beside {
                    identity_grid(ui, &rows);
                }
            });
            if !beside {
                ui.horizontal(|ui| {
                    ui.add_space(56.);
                    identity_grid(ui, &rows);
                });
            }
        });
}
fn identity_grid(ui: &mut Ui, rows: &[(&str, String); 3]) {
    let p = theme::palette(ui.visuals());
    egui::Grid::new("health-identity")
        .num_columns(2)
        .spacing([12., 2.])
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.label(RichText::new(*label).size(12.).color(p.text2));
                ui.label(RichText::new(value).size(12.));
                ui.end_row();
            }
        });
}
fn state_icon(painter: &egui::Painter, rect: egui::Rect, state: &Overall, color: Color32) {
    let clock = |painter: &egui::Painter| {
        painter.circle_stroke(
            rect.center(),
            9. * rect.width() / 24.,
            egui::Stroke::new(2. * rect.width() / 24., color),
        );
        widgets::strokes(
            painter,
            rect,
            &[&[(12., 7.), (12., 12.), (15., 14.)]],
            2.,
            color,
        );
    };
    match state {
        Overall::Healthy => widgets::strokes(
            painter,
            rect,
            &[&[(5., 12.5), (9.5, 17.), (19., 7.5)]],
            2.2,
            color,
        ),
        Overall::Alerts(_) => {
            widgets::strokes(
                painter,
                rect,
                &[
                    &[(12., 3.), (21.5, 20.), (2.5, 20.), (12., 3.)],
                    &[(12., 10.), (12., 14.)],
                ],
                2.,
                color,
            );
            widgets::dot(painter, rect, (12., 17.), 2.4, color);
        }
        Overall::Unavailable(_) => {
            painter.circle_stroke(
                rect.center(),
                9. * rect.width() / 24.,
                egui::Stroke::new(2. * rect.width() / 24., color),
            );
            widgets::strokes(painter, rect, &[&[(12., 8.), (12., 13.)]], 2., color);
            widgets::dot(painter, rect, (12., 16.), 2.4, color);
        }
        Overall::Stale | Overall::Waiting => clock(painter),
    }
}

fn consolidation(ui: &mut Ui, c: &Value) {
    let p = theme::palette(ui.visuals());
    let get = |key: &str| c[key].as_u64();
    // Sem as quatro contagens a proporção é desconhecida: só o trilho.
    let parts = match (
        get("completed"),
        get("running"),
        get("pending"),
        get("failed"),
    ) {
        (Some(done), Some(running), Some(pending), Some(failed)) => vec![
            (done as f64, p.acc),
            (running as f64, p.series2),
            (pending as f64, p.text2),
            (failed as f64, p.bad),
        ],
        _ => Vec::new(),
    };
    widgets::stacked_bar(ui, &parts, widgets::THICK, "Proporção das consolidações");
    widgets::facts(
        ui,
        &[
            Fact::new("Concluídas", number(&c["completed"])),
            Fact::new(
                "Pendentes · em execução",
                format!("{} · {}", number(&c["pending"]), number(&c["running"])),
            ),
            Fact::new("Com falha", number(&c["failed"]))
                .color(get("failed").filter(|&n| n > 0).map(|_| p.bad)),
        ],
    );
}

fn write_queue(ui: &mut Ui, q: &Value) {
    let p = theme::palette(ui.visuals());
    let (queued, capacity) = (q[0].as_u64(), q[1].as_u64());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &number(&q[0]),
        0.,
        egui::TextFormat::simple(Type::Metric.font(), p.text),
    );
    job.append(
        &format!(" de {} posições", number(&q[1])),
        0.,
        egui::TextFormat::simple(Type::Body.font(), p.text2),
    );
    ui.label(job);
    let ratio = match (queued, capacity) {
        (Some(queued), Some(capacity)) if capacity > 0 => Some(queued as f64 / capacity as f64),
        _ => None,
    };
    let high = ratio.is_some_and(|r| r >= 0.8);
    widgets::meter(
        ui,
        ratio.map(|r| r as f32),
        widgets::THICK,
        if high { p.warn } else { p.acc },
        "Uso da fila de escrita",
    );
    let (sentence, color) = match (ratio, queued) {
        (None, _) => ("Ocupação desconhecida".to_owned(), p.text2),
        (Some(_), Some(0)) => ("Sem pressão de escrita".to_owned(), p.text2),
        (Some(r), _) => (
            format!("{}% ocupada", decimal(r * 100., 1)),
            if high { p.warn } else { p.text2 },
        ),
    };
    ui.label(RichText::new(sentence).size(12.).color(color));
}

fn providers(ui: &mut Ui, v: &Value) {
    let p = theme::palette(ui.visuals());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 8.;
        for (role, name) in [("llm", "LLM"), ("embedding", "Embedding")] {
            let provider = &v[role];
            let status = provider["status"].as_str();
            let color = match status {
                Some("error" | "unhealthy" | "failed") => p.bad,
                Some("ok" | "healthy") => p.acc,
                _ => p.text2,
            };
            provider_row(
                ui,
                &format!("{name} · {}", text(provider, "provider")),
                text(provider, "model"),
                status.unwrap_or("—"),
                color,
            );
        }
    });
    let workloads = array(&v["llm_workloads"]);
    widgets::disclosure_link(
        ui,
        "workloads",
        &format!(
            "Uso por atividade ({})",
            crate::model::number(workloads.len())
        ),
        |ui| {
            ui.small("Contadores passivos desde o início do servidor. Estimativas e tokens reportados são medidas distintas.");
            let mut seen = std::collections::HashSet::new();
            for (idx, row) in workloads.iter().take(100).enumerate() {
                ui.push_id(row_key(&mut seen, row["workload"].as_str(), idx), |ui| {
                    ui.strong(text(row, "workload"));
                    count_grid(
                        ui,
                        "workload",
                        row,
                        &[
                            ("calls", "Chamadas"),
                            ("errors", "Falhas"),
                            ("reported_input_tokens", "Entrada reportada"),
                            ("reported_output_tokens", "Saída reportada"),
                            ("estimated_input_tokens", "Entrada estimada"),
                            ("estimated_output_tokens", "Saída estimada"),
                        ],
                    );
                });
            }
        },
    );
}
/// Ponto de estado, nome e modelo em duas linhas e o status à direita.
fn provider_row(ui: &mut Ui, title: &str, model: &str, status: &str, color: Color32) {
    let p = theme::palette(ui.visuals());
    let status_galley = ui
        .painter()
        .layout_no_wrap(status.to_owned(), Type::Caption.font(), color);
    let width = ui.available_width();
    let left = 7. + 10.;
    let room = (width - left - status_galley.size().x - 10.).max(0.);
    let line = |text: &str, font: egui::FontId, color: Color32| {
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(room);
        ui.painter().layout_job(job)
    };
    let name = line(title, Type::Body.font(), p.text);
    let detail = line(model, Type::Caption.font(), p.text2);
    let height = name.size().y + detail.size().y;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    let spoken = format!("{title} · {model} · {status}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &spoken));
    let painter = ui.painter();
    painter.circle_filled(pos2(rect.left() + 3.5, rect.center().y), 3.5, color);
    let name_height = name.size().y;
    painter.galley(pos2(rect.left() + left, rect.top()), name, p.text);
    painter.galley(
        pos2(rect.left() + left, rect.top() + name_height),
        detail,
        p.text2,
    );
    painter.galley(
        pos2(
            rect.right() - status_galley.size().x,
            rect.center().y - status_galley.size().y / 2.,
        ),
        status_galley,
        color,
    );
}

fn indices(ui: &mut Ui, d: &Value, p: &theme::Palette) {
    let mut rows = vec![
        Fact::new(
            "Páginas sem embedding",
            number(&d["latest_pages_missing_embeddings"]),
        ),
        Fact::new(
            "Embeddings com falha",
            number(&d["embed_failures_unresolved"]),
        ),
        Fact::new(
            "Links não resolvidos",
            number(&d["unresolved_links_from_latest_pages"]),
        ),
    ];
    match (d["pages_rows"].as_u64(), d["pages_fts_rows"].as_u64()) {
        (Some(pages), Some(indexed)) if pages == indexed => rows.push(
            Fact::new(
                "Índice textual",
                format!("em dia · {}", crate::model::number(pages as usize)),
            )
            .color(Some(p.acc)),
        ),
        (pages, indexed) => {
            // Só as duas contagens conhecidas e diferentes são um atraso.
            let late = (pages.is_some() && indexed.is_some()).then_some(p.warn);
            rows.push(Fact::new("Versões de página", number(&d["pages_rows"])).color(late));
            rows.push(
                Fact::new("Versões no índice textual", number(&d["pages_fts_rows"])).color(late),
            );
        }
    }
    widgets::facts(ui, &rows);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::Error,
        widgets::tests::{THEMES, click_sized, context, node, nodes, rect_of, run_sized, texts},
    };
    use egui::accesskit::Role;
    use serde_json::json;
    use std::sync::Arc;

    const SIZE: [f32; 2] = [900., 2400.];

    fn shared() -> Arc<Shared> {
        Shared::unstarted(crate::model::Config::default())
    }
    fn page(
        ctx: &egui::Context,
        shared: &Shared,
        s: &Snapshot,
        refreshing: bool,
        paused: bool,
        now: DateTime<Utc>,
    ) -> egui::FullOutput {
        run_sized(ctx, SIZE, vec![], |ui| {
            show(ui, shared, s, refreshing, paused, now)
        })
    }
    fn labels(out: &egui::FullOutput) -> Vec<String> {
        out.platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == Role::Label)
            .filter_map(|(_, n)| n.value().map(str::to_owned))
            .collect()
    }
    /// Uma leitura em dia, completa e saudável.
    fn healthy(now: DateTime<Utc>) -> Snapshot {
        Snapshot {
            at: Some(now),
            identity: Some(health::Identity {
                version: "3.3.1".into(),
                level: "anonymous".into(),
                distinguishes_operators: false,
            }),
            status: Some(json!({
                "version": "3.3.1",
                "counts": {"pages_latest": 1176, "sessions": 2285, "observations": 821703},
                "consolidation": {"completed": 116, "pending": 0, "running": 0, "failed": 0},
                "write_queue": [0, 1024],
                "providers": {
                    "llm": {"provider": "anthropic", "model": "claude-haiku-5-5", "status": "ok"},
                    "embedding": {"provider": "local", "model": "all-MiniLM-L6-v2", "status": "ok"},
                    "llm_workloads": [{"workload": "consolidate", "calls": 3}]
                },
                "storage": {"database_bytes": 1395864371u64, "reclaimable_bytes": 4096, "data_dir_free_bytes": 863288426496u64},
                "derived": {
                    "latest_pages_missing_embeddings": 0,
                    "embed_failures_unresolved": 0,
                    "unresolved_links_from_latest_pages": 12,
                    "pages_rows": 3410,
                    "pages_fts_rows": 3410
                }
            })),
            ..Default::default()
        }
    }
    const ERRORS: [Error; 11] = [
        Error::Invalid(String::new()),
        Error::Authentication,
        Error::Forbidden,
        Error::Missing,
        Error::Conflict,
        Error::RateLimited,
        Error::Unavailable,
        Error::TooLarge,
        Error::Format,
        Error::Http(503),
        Error::Cancelled,
    ];

    #[test]
    fn overall_state_for_every_kind_of_reading() {
        let now = Utc::now();
        let ago = |secs| Some(now - chrono::Duration::seconds(secs));
        assert_eq!(overall(&Snapshot::default(), now), Overall::Waiting);
        assert_eq!(
            overall(
                &Snapshot {
                    at: ago(91),
                    ..healthy(now)
                },
                now
            ),
            Overall::Stale
        );
        assert_eq!(
            overall(
                &Snapshot {
                    at: ago(90),
                    ..healthy(now)
                },
                now
            ),
            Overall::Healthy,
            "90 s is still fresh"
        );
        assert_eq!(overall(&healthy(now), now), Overall::Healthy);
        for error in ERRORS {
            // Um erro novo tem de entrar em ERRORS.
            match error {
                Error::Invalid(_)
                | Error::Authentication
                | Error::Forbidden
                | Error::Missing
                | Error::Conflict
                | Error::RateLimited
                | Error::Unavailable
                | Error::TooLarge
                | Error::Format
                | Error::Http(_)
                | Error::Cancelled => {}
            }
            let s = Snapshot {
                at: ago(0),
                error: Some(error.clone()),
                ..Default::default()
            };
            assert_eq!(overall(&s, now), Overall::Unavailable(error.to_string()));
            // Atrasada vence o erro, como no resumo do menu.
            let s = Snapshot { at: ago(91), ..s };
            assert_eq!(overall(&s, now), Overall::Stale, "{error:?}");
        }
        let with = |patch: serde_json::Value| {
            let mut s = healthy(now);
            let status = s.status.as_mut().unwrap();
            for (key, value) in patch.as_object().unwrap() {
                status[key] = value.clone();
            }
            overall(&s, now)
        };
        for (case, patch, want) in [
            (
                "failed consolidation",
                json!({"consolidation": {"failed": 2}}),
                1,
            ),
            ("queue at 80%", json!({"write_queue": [820, 1024]}), 1),
            ("queue below 80%", json!({"write_queue": [819, 1024]}), 0),
            (
                "disk under 512 MiB",
                json!({"storage": {"data_dir_free_bytes": 1024}}),
                1,
            ),
            (
                "both providers failing",
                json!({"providers": {"llm": {"status": "error"}, "embedding": {"status": "unhealthy"}}}),
                2,
            ),
            (
                "three at once",
                json!({"consolidation": {"failed": 1}, "write_queue": [1024, 1024], "storage": {"data_dir_free_bytes": 0}}),
                3,
            ),
        ] {
            let want = if want == 0 {
                Overall::Healthy
            } else {
                Overall::Alerts(want)
            };
            assert_eq!(with(patch), want, "{case}");
        }
    }

    #[test]
    fn status_card_names_every_state_with_its_tone_in_both_themes() {
        let now = Utc::now();
        let mut alerts = healthy(now);
        alerts.status.as_mut().unwrap()["consolidation"]["failed"] = json!(1);
        let cases = [
            (Snapshot::default(), "Aguardando"),
            (
                Snapshot {
                    at: Some(now - chrono::Duration::seconds(200)),
                    ..healthy(now)
                },
                "Leitura atrasada",
            ),
            (
                Snapshot {
                    at: Some(now),
                    error: Some(Error::Forbidden),
                    ..Default::default()
                },
                "Indisponível",
            ),
            (alerts, "Com alertas"),
            (healthy(now), "Saudável"),
        ];
        for (theme, p) in THEMES {
            for (s, title) in &cases {
                let ctx = context(theme);
                let out = page(&ctx, &shared(), s, false, false, now);
                let state = overall(s, now);
                assert_eq!(state.title(), *title);
                node(&out, Role::Label, title);
                node(&out, Role::Label, &state.subtitle());
                let fill = match state {
                    Overall::Healthy => p.sel,
                    Overall::Alerts(_) | Overall::Stale => p.warn.gamma_multiply(0.12),
                    Overall::Unavailable(_) => p.bad.gamma_multiply(0.08),
                    Overall::Waiting => p.surf2,
                };
                let circles: Vec<_> = crate::widgets::tests::shapes(&out)
                    .into_iter()
                    .filter_map(|s| match s {
                        egui::Shape::Circle(c) if c.radius == 20. => Some(c.fill),
                        _ => None,
                    })
                    .collect();
                assert_eq!(circles, vec![fill], "{theme:?} {title}");
            }
        }
        let ctx = context(egui::Theme::Dark);
        let out = page(&ctx, &shared(), &cases[1].0.clone(), false, false, now);
        node(
            &out,
            Role::Label,
            "A última coleta tem mais de 90 s; os números abaixo podem estar desatualizados.",
        );
        let out = page(&ctx, &shared(), &cases[2].0.clone(), false, false, now);
        node(
            &out,
            Role::Label,
            "A identidade conectada não tem permissão para este recurso.",
        );
        let out = page(&ctx, &shared(), &healthy(now), false, false, now);
        for value in ["3.3.1", "Anônimo", "Não"] {
            node(&out, Role::Label, value);
        }
        let out = page(&ctx, &shared(), &Snapshot::default(), false, false, now);
        assert_eq!(
            nodes(&out, Role::Label, "—").len(),
            3,
            "unknown identity: version, access and operators are —"
        );
    }

    /// Todos os valores dos cartões, como o leitor de tela os diz.
    const FACTS: [&str; 14] = [
        "Páginas atuais",
        "Sessões",
        "Eventos",
        "Concluídas",
        "Pendentes · em execução",
        "Com falha",
        "Banco de dados",
        "Potencial recuperável",
        "Disco livre no servidor",
        "Páginas sem embedding",
        "Embeddings com falha",
        "Links não resolvidos",
        "Versões de página",
        "Versões no índice textual",
    ];

    #[test]
    fn denied_or_missing_fields_are_a_dash_never_zero() {
        let now = Utc::now();
        for (case, s) in [
            ("waiting", Snapshot::default()),
            (
                "permission denied",
                Snapshot {
                    at: Some(now),
                    error: Some(Error::Forbidden),
                    ..Default::default()
                },
            ),
            (
                "empty status",
                Snapshot {
                    at: Some(now),
                    status: Some(json!({"version": "3.3.1", "counts": {}})),
                    ..Default::default()
                },
            ),
        ] {
            let ctx = context(egui::Theme::Light);
            let out = page(&ctx, &shared(), &s, false, false, now);
            let labels = labels(&out);
            for fact in FACTS {
                let want = if fact == "Pendentes · em execução" {
                    format!("{fact}: — · —")
                } else {
                    format!("{fact}: —")
                };
                assert!(labels.contains(&want), "{case}: {want} in {labels:?}");
            }
            assert!(
                !labels
                    .iter()
                    .any(|l| l.ends_with(": 0") || l.starts_with("0 ")),
                "{case}: nothing reads as zero: {labels:?}"
            );
            for text in [
                "— de — posições",
                "Ocupação desconhecida",
                "LLM · — · — · —",
                "Embedding · — · — · —",
                "Uso por atividade (0)",
            ] {
                assert!(
                    labels.iter().any(|l| l == text) || !nodes(&out, Role::Button, text).is_empty(),
                    "{case}: {text}"
                );
            }
            assert_eq!(
                node(&out, Role::ProgressIndicator, "Uso da fila de escrita").numeric_value(),
                None,
                "{case}: an unknown queue is not an empty one"
            );
        }
    }

    #[test]
    fn zeros_from_the_server_are_shown_as_zero_and_partial_fields_as_dash() {
        let now = Utc::now();
        let s = Snapshot {
            at: Some(now),
            status: Some(json!({
                "version": "3.3.1",
                "counts": {"pages_latest": 0},
                "consolidation": {"completed": 0, "pending": 0, "running": 0, "failed": 0},
                "write_queue": [0, 1024],
                "derived": {"embed_failures_unresolved": 0}
            })),
            ..Default::default()
        };
        let ctx = context(egui::Theme::Dark);
        let out = page(&ctx, &shared(), &s, false, false, now);
        for text in [
            "Páginas atuais: 0",
            "Sessões: —",
            "Eventos: —",
            "Concluídas: 0",
            "Pendentes · em execução: 0 · 0",
            "Com falha: 0",
            "Embeddings com falha: 0",
            "Páginas sem embedding: —",
            "Banco de dados: —",
            "0 de 1.024 posições",
            "Sem pressão de escrita",
        ] {
            node(&out, Role::Label, text);
        }
        assert_eq!(
            node(&out, Role::ProgressIndicator, "Uso da fila de escrita").numeric_value(),
            Some(0.)
        );
    }

    #[test]
    fn consolidation_bar_needs_all_four_counts() {
        let now = Utc::now();
        for (theme, p) in THEMES {
            for (consolidation, painted) in [
                (
                    json!({"completed": 116, "pending": 0, "running": 0, "failed": 0}),
                    true,
                ),
                (json!({"completed": 116, "pending": 0, "running": 0}), false),
                (json!({}), false),
                (
                    json!({"completed": 0, "pending": 0, "running": 0, "failed": 0}),
                    false,
                ),
            ] {
                let mut s = healthy(now);
                s.status.as_mut().unwrap()["consolidation"] = consolidation.clone();
                let ctx = context(theme);
                let out = page(&ctx, &shared(), &s, false, false, now);
                // A fila do cenário está vazia: o único acc de 8 px é a barra.
                let segments = crate::widgets::tests::rects(&out)
                    .into_iter()
                    .filter(|r| r.fill == p.acc && r.rect.height() == crate::widgets::THICK)
                    .count();
                assert_eq!(segments, usize::from(painted), "{theme:?} {consolidation}");
                node(&out, Role::ProgressIndicator, "Proporção das consolidações");
            }
        }
    }

    #[test]
    fn full_reading_fills_every_card() {
        let now = Utc::now();
        let ctx = context(egui::Theme::Light);
        let out = page(&ctx, &shared(), &healthy(now), false, false, now);
        for text in [
            "Páginas atuais: 1.176",
            "Sessões: 2.285",
            "Eventos: 821.703",
            "Concluídas: 116",
            "Pendentes · em execução: 0 · 0",
            "Com falha: 0",
            "0 de 1.024 posições",
            "Sem pressão de escrita",
            "LLM · anthropic · claude-haiku-5-5 · ok",
            "Embedding · local · all-MiniLM-L6-v2 · ok",
            "Banco de dados: 1,3 GiB",
            "Potencial recuperável: 4,0 KiB",
            "Disco livre no servidor: 804,0 GiB",
            "Páginas sem embedding: 0",
            "Embeddings com falha: 0",
            "Links não resolvidos: 12",
            "Índice textual: em dia · 3.410",
            CAPTION,
        ] {
            node(&out, Role::Label, text);
        }
        node(&out, Role::Button, "Uso por atividade (1)");
        for title in CARDS {
            node(&out, Role::Pane, title);
        }
    }

    #[test]
    fn text_index_is_up_to_date_only_when_both_counts_are_known_and_equal() {
        let now = Utc::now();
        for (theme, p) in THEMES {
            for (pages, indexed, want, color) in [
                (
                    json!(3410),
                    json!(3410),
                    vec!["Índice textual: em dia · 3.410"],
                    Some(p.acc),
                ),
                (
                    json!(0),
                    json!(0),
                    vec!["Índice textual: em dia · 0"],
                    Some(p.acc),
                ),
                (
                    json!(3412),
                    json!(3410),
                    vec![
                        "Versões de página: 3.412",
                        "Versões no índice textual: 3.410",
                    ],
                    Some(p.warn),
                ),
                (
                    json!(3412),
                    json!(null),
                    vec!["Versões de página: 3.412", "Versões no índice textual: —"],
                    None,
                ),
                (
                    json!(null),
                    json!(null),
                    vec!["Versões de página: —", "Versões no índice textual: —"],
                    None,
                ),
            ] {
                let mut s = healthy(now);
                s.status.as_mut().unwrap()["derived"]["pages_rows"] = pages;
                s.status.as_mut().unwrap()["derived"]["pages_fts_rows"] = indexed;
                let ctx = context(theme);
                let out = page(&ctx, &shared(), &s, false, false, now);
                for text in &want {
                    node(&out, Role::Label, text);
                }
                let painted = texts(&out);
                for text in &want {
                    let value = text.split(": ").nth(1).unwrap();
                    let expected = color.unwrap_or(p.text);
                    assert!(
                        painted.contains(&(value.to_owned(), expected)),
                        "{theme:?} {text}: {expected:?}"
                    );
                }
                if want.len() == 2 {
                    assert!(nodes(&out, Role::Label, "Índice textual: em dia · 3.410").is_empty());
                }
            }
        }
    }

    #[test]
    fn write_queue_headline_meter_and_pressure() {
        let now = Utc::now();
        for (theme, p) in THEMES {
            for (queue, headline, sentence, value, color) in [
                (
                    json!([0, 1024]),
                    "0 de 1.024 posições",
                    "Sem pressão de escrita",
                    Some(0.),
                    p.text2,
                ),
                (
                    json!([512, 1024]),
                    "512 de 1.024 posições",
                    "50,0% ocupada",
                    Some(50.),
                    p.text2,
                ),
                (
                    json!([819, 1024]),
                    "819 de 1.024 posições",
                    "80,0% ocupada",
                    Some(79.98046875),
                    p.text2,
                ),
                (
                    json!([820, 1024]),
                    "820 de 1.024 posições",
                    "80,1% ocupada",
                    Some(80.078125),
                    p.warn,
                ),
                (
                    json!([1024, 1024]),
                    "1.024 de 1.024 posições",
                    "100,0% ocupada",
                    Some(100.),
                    p.warn,
                ),
                (
                    json!([5, 0]),
                    "5 de 0 posições",
                    "Ocupação desconhecida",
                    None,
                    p.text2,
                ),
                (
                    json!([5]),
                    "5 de — posições",
                    "Ocupação desconhecida",
                    None,
                    p.text2,
                ),
                (
                    json!(null),
                    "— de — posições",
                    "Ocupação desconhecida",
                    None,
                    p.text2,
                ),
            ] {
                let mut s = healthy(now);
                s.status.as_mut().unwrap()["write_queue"] = queue.clone();
                let ctx = context(theme);
                let out = page(&ctx, &shared(), &s, false, false, now);
                node(&out, Role::Label, headline);
                node(&out, Role::Label, sentence);
                let meter = node(&out, Role::ProgressIndicator, "Uso da fila de escrita");
                assert_eq!(
                    meter.numeric_value().map(|v| (v * 1000.).round() / 1000.),
                    value.map(|v: f64| (v * 1000.).round() / 1000.),
                    "{theme:?} {queue}"
                );
                assert!(
                    texts(&out).contains(&(sentence.to_owned(), color)),
                    "{theme:?} {queue}: sentence color"
                );
            }
        }
    }

    #[test]
    fn providers_show_status_with_its_tone_and_the_activity_count() {
        let now = Utc::now();
        for (theme, p) in THEMES {
            for (status, color) in [
                (json!("ok"), p.acc),
                (json!("healthy"), p.acc),
                (json!("error"), p.bad),
                (json!("unhealthy"), p.bad),
                (json!("failed"), p.bad),
                (json!("disabled"), p.text2),
                (json!(null), p.text2),
            ] {
                let mut s = healthy(now);
                s.status.as_mut().unwrap()["providers"]["embedding"]["status"] = status.clone();
                let ctx = context(theme);
                let out = page(&ctx, &shared(), &s, false, false, now);
                let shown = status.as_str().unwrap_or("—");
                node(
                    &out,
                    Role::Label,
                    &format!("Embedding · local · all-MiniLM-L6-v2 · {shown}"),
                );
                assert!(
                    texts(&out).contains(&(shown.to_owned(), color)),
                    "{theme:?} {status}"
                );
            }
        }
        let mut s = healthy(now);
        s.status.as_mut().unwrap()["providers"]["llm_workloads"] = json!([
            {"workload": "consolidate", "calls": 3, "errors": 0},
            {"workload": "curate", "calls": 1},
            {"workload": "consolidate", "calls": 9}
        ]);
        let ctx = context(egui::Theme::Dark);
        let mut add = |ui: &mut Ui| show(ui, &shared(), &s, false, false, now);
        let out = run_sized(&ctx, SIZE, vec![], &mut add);
        let toggle = rect_of(&node(&out, Role::Button, "Uso por atividade (3)"));
        assert!(
            nodes(&out, Role::Label, "Chamadas").is_empty(),
            "closed by default"
        );
        click_sized(&ctx, SIZE, toggle.center(), &mut add);
        let out = run_sized(&ctx, SIZE, vec![], &mut add);
        assert_eq!(
            nodes(&out, Role::Label, "Chamadas").len(),
            3,
            "one grid per workload"
        );
        assert_eq!(nodes(&out, Role::Label, "curate").len(), 1);
    }

    #[test]
    fn alerts_and_partial_collection_become_warning_strips() {
        let now = Utc::now();
        let mut s = healthy(now);
        s.status.as_mut().unwrap()["consolidation"]["failed"] = json!(2);
        s.auxiliary_errors =
            vec!["Métricas: Servidor indisponível ou tempo limite atingido.".into()];
        let ctx = context(egui::Theme::Light);
        let out = page(&ctx, &shared(), &s, false, false, now);
        node(&out, Role::Label, "Consolidações com falha.");
        node(
            &out,
            Role::Label,
            "Coleta parcial. Métricas: Servidor indisponível ou tempo limite atingido.",
        );
        node(&out, Role::Label, "Com alertas");
        // Sem status (erro), nenhum alerta de status vira faixa; as parciais seguem.
        let s = Snapshot {
            at: Some(now),
            error: Some(Error::Unavailable),
            auxiliary_errors: vec![
                "Identidade: Servidor indisponível ou tempo limite atingido.".into(),
            ],
            ..Default::default()
        };
        let out = page(&ctx, &shared(), &s, false, false, now);
        node(&out, Role::Label, "Indisponível");
        node(
            &out,
            Role::Label,
            "Coleta parcial. Identidade: Servidor indisponível ou tempo limite atingido.",
        );
        assert!(nodes(&out, Role::Label, "Saúde do servidor indisponível.").is_empty());
    }

    #[test]
    fn disclosures_count_series_and_keep_every_raw_field_reachable() {
        let now = Utc::now();
        let metric = |series: &str| health::Metric {
            series: series.into(),
            value: 42.,
        };
        for (metrics, meta) in [
            (vec![], "0 séries"),
            (vec![metric("ai_memory_a")], "1 série"),
            (
                vec![metric("ai_memory_a"), metric("ai_memory_b")],
                "2 séries",
            ),
        ] {
            let s = Snapshot {
                metrics,
                ..healthy(now)
            };
            let ctx = context(egui::Theme::Dark);
            let out = page(&ctx, &shared(), &s, false, false, now);
            let row = node(
                &out,
                Role::Button,
                "Métricas acumuladas desde o início do servidor",
            );
            assert_eq!(row.is_expanded(), Some(false));
            assert!(texts(&out).iter().any(|(t, _)| t == meta), "{meta}");
        }
        let mut s = healthy(now);
        s.status.as_mut().unwrap()["field_only_in_raw"] = json!("visível");
        s.features = Some(json!({"web": true}));
        let ctx = context(egui::Theme::Light);
        let mut add = |ui: &mut Ui| show(ui, &shared(), &s, false, false, now);
        let out = run_sized(&ctx, SIZE, vec![], &mut add);
        assert!(nodes(&out, Role::Label, "field_only_in_raw: visível").is_empty());
        let raw = rect_of(&node(
            &out,
            Role::Button,
            "Dados brutos informados pelo servidor",
        ));
        click_sized(&ctx, SIZE, raw.center(), &mut add);
        let out = run_sized(&ctx, SIZE, vec![], &mut add);
        for text in ["field_only_in_raw: visível", "version: 3.3.1", "web: true"] {
            node(&out, Role::Label, text);
        }
        let s = Snapshot::default();
        let mut add = |ui: &mut Ui| show(ui, &shared(), &s, false, false, now);
        let ctx = context(egui::Theme::Light);
        let out = run_sized(&ctx, SIZE, vec![], &mut add);
        let raw = rect_of(&node(
            &out,
            Role::Button,
            "Dados brutos informados pelo servidor",
        ));
        click_sized(&ctx, SIZE, raw.center(), &mut add);
        let out = run_sized(&ctx, SIZE, vec![], &mut add);
        assert_eq!(nodes(&out, Role::Label, "Não informado.").len(), 2);
        let metrics = rect_of(&node(
            &out,
            Role::Button,
            "Métricas acumuladas desde o início do servidor",
        ));
        click_sized(&ctx, SIZE, metrics.center(), &mut add);
        let out = run_sized(&ctx, SIZE, vec![], &mut add);
        node(&out, Role::Label, "Nenhuma métrica disponível.");
    }

    #[test]
    fn toolbar_tells_the_collection_age_and_the_cycle() {
        let now = Utc::now();
        let cycle = "a cada 30 s, mesmo com a janela fechada";
        for (at, want) in [
            (None, format!("Aguardando a primeira coleta · {cycle}")),
            (Some(0), format!("Coletado há 0 s · {cycle}")),
            (Some(12), format!("Coletado há 12 s · {cycle}")),
            (Some(150), format!("Coletado há 2 min · {cycle}")),
        ] {
            let s = Snapshot {
                at: at.map(|secs| now - chrono::Duration::seconds(secs)),
                ..Default::default()
            };
            let ctx = context(egui::Theme::Dark);
            let out = page(&ctx, &shared(), &s, false, false, now);
            node(&out, Role::Label, &want);
        }
    }

    #[test]
    fn refresh_button_states_and_one_request_per_enabled_click() {
        let now = Utc::now();
        for (refreshing, paused, label, disabled) in [
            (false, false, "Atualizar agora", false),
            (true, false, "Atualizando…", true),
            (false, true, "Atualizar agora", true),
            (true, true, "Atualizando…", true),
        ] {
            let shared = shared();
            let s = healthy(now);
            let ctx = context(egui::Theme::Light);
            let mut add = |ui: &mut Ui| show(ui, &shared, &s, refreshing, paused, now);
            let out = run_sized(&ctx, SIZE, vec![], &mut add);
            let button = node(&out, Role::Button, label);
            assert_eq!(button.is_disabled(), disabled, "{label} paused={paused}");
            click_sized(&ctx, SIZE, rect_of(&button).center(), &mut add);
            click_sized(&ctx, SIZE, rect_of(&button).center(), &mut add);
            assert_eq!(
                shared
                    .health_asked
                    .load(std::sync::atomic::Ordering::Acquire),
                if disabled { 0 } else { 2 },
                "{label} paused={paused}: each enabled click asks once"
            );
        }
    }
}
