//! Página Docker (Docker.dc.html): segmentado Contêineres · Imagens ·
//! Processos do <HOST> com a busca, a linha-resumo com "Só em execução", a
//! tabela de contêineres ordenável com o inspetor e as tabelas de imagens e de
//! processos. A página controla a própria rolagem (fora da rolagem da área
//! principal) e cabe na largura sem rolar para o lado. Só consulta.
//!
//! Regras desta tela:
//! - Contadores do segmentado, sem filtro: Contêineres = listados pelo Docker;
//!   Imagens = do inventário; "—" sem coleta ou com o Docker indisponível.
//! - Linha-resumo: soma dos contêineres em execução (`docker_total`). Sem
//!   coleta, sem Docker ou sem a métrica de algum deles, "—"; nunca 0.
//! - CPU: 100% = 1 núcleo, e a barra enche em 1 núcleo (acima disso, cheia).
//!   Memória: a barra é o uso ÷ o limite do contêiner.
//! - Ordenação: Nome, CPU, Memória e Tarefas. O 1º clique numa métrica ordena
//!   do maior ao menor; no Nome, de A a Z (sem diferenciar maiúsculas); o
//!   clique seguinte inverte. Quem não tem o valor (parado, sem `docker stats`
//!   ou sem tarefas) fica no fim nas duas direções, em execução primeiro e por
//!   nome. Empates: nome e ID. Padrão: CPU decrescente, o que põe os em
//!   execução primeiro. A ordem sai só dos dados da coleta: a mesma coleta dá
//!   a mesma ordem, qualquer que seja a ordem em que o Docker listou.
//! - Seleção = ID do contêiner. Padrão: o monitorado (`config.container`) se
//!   estiver na lista filtrada, senão o primeiro. Se o selecionado some ou
//!   sai do filtro, volta ao padrão; lista vazia = sem inspetor.
//! - Largura de conteúdo ≥ `SIDE_BY_SIDE`: inspetor de `INSPECTOR` à
//!   direita. Abaixo disso, o inspetor vem depois da tabela, na mesma
//!   rolagem. A coluna Portas só aparece com a tabela a partir de
//!   `PORTS_FROM` (no inspetor, sempre). Tudo sai da largura do frame.
use crate::{
    app::{ago, process_matches, short_id},
    model::{
        Container, DockerImage, HostProcess, ResourceSnapshot, bytes, decimal, decimal_up_to,
        number,
    },
    theme,
    widgets::{self, Button, Fact, Segment, THIN, Tone, Type, space},
};
use chrono::{DateTime, Utc};
use eframe::egui::{
    self, Align, Color32, FontId, Label, Layout, Rect, Response, RichText, Sense, Stroke, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType, accesskit::SortDirection, pos2, vec2,
};
use std::cmp::Ordering;

/// Larguras das colunas fixas da tabela de contêineres, com o respiro de
/// `PAD` de cada lado; Nome fica com o resto.
const CPU_WIDTH: f32 = 150.;
const MEMORY_WIDTH: f32 = 170.;
const TASKS_WIDTH: f32 = 80.;
const PORTS_WIDTH: f32 = 130.;
/// Nome mínimo com a coluna Portas; sem ela, Nome não desce de `FLEX_MIN`.
const NAME_MIN: f32 = 180.;
const FLEX_MIN: f32 = 120.;
/// Largura de tabela a partir da qual a coluna Portas aparece.
const PORTS_FROM: f32 = NAME_MIN + CPU_WIDTH + MEMORY_WIDTH + TASKS_WIDTH + PORTS_WIDTH;
const INSPECTOR: f32 = 320.;
/// Largura de conteúdo a partir da qual o inspetor fica ao lado: a tabela
/// com Portas, 1 da borda e o inspetor.
pub(crate) const SIDE_BY_SIDE: f32 = PORTS_FROM + 1. + INSPECTOR;
const HEAD: f32 = 32.;
const ROW: f32 = 44.;
const PROCESS_ROW: f32 = 36.;
const INSPECTOR_ROW: f32 = 22.;
const PAD: f32 = 12.;
/// Espaço reservado ao valor antes da barra (CPU, memória).
const CPU_VALUE: f32 = 52.;
const MEMORY_VALUE: f32 = 76.;
const SUMMARY: f32 = 44.;
/// Espaço entre os itens da linha-resumo; o justo, quando o de 22 não deixa
/// o interruptor na mesma linha.
const SUMMARY_GAP: f32 = 22.;
const SUMMARY_TIGHT: f32 = 14.;
const SEARCH: &str = "Nome, imagem ou ID";
const UNITS: &str = "CPU: 100% = 1 núcleo · memória sem cache inativo";
const IMAGES_NOTE: &str = "Imagens associadas a contêineres parados continuam em uso.";
const SIZE_NOTE: &str = "Tamanho lógico por imagem; camadas compartilhadas impedem somar estes valores como espaço real em disco.";
const ONLY_RUNNING: &str = "Só em execução";
pub(crate) const TASKS_HINT: &str = "Processos + threads Linux";
const IMAGES_HINT: &str = "IDs únicos; tags agrupadas";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tab {
    Containers,
    Images,
    Processes,
}

/// Colunas da tabela de contêineres; todas ordenam, menos Portas.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Col {
    Name,
    Cpu,
    Memory,
    Tasks,
    Ports,
}
impl Col {
    fn title(self) -> &'static str {
        match self {
            Col::Name => "Nome",
            Col::Cpu => "CPU",
            Col::Memory => "Memória / limite",
            Col::Tasks => "Tarefas",
            Col::Ports => "Portas",
        }
    }
    fn sorts(self) -> bool {
        self != Col::Ports
    }
    /// A métrica da coluna; `None` = sem valor (vai ao fim).
    fn metric(self, c: &Container) -> Option<f64> {
        let u = c.usage.as_ref()?;
        match self {
            Col::Cpu => Some(u.cpu),
            Col::Memory => Some(u.memory as f64),
            Col::Tasks => u.tasks.map(|t| t as f64),
            Col::Name | Col::Ports => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Sort {
    col: Col,
    descending: bool,
}
impl Default for Sort {
    fn default() -> Self {
        Self {
            col: Col::Cpu,
            descending: true,
        }
    }
}
impl Sort {
    /// Clique num cabeçalho: o ativo inverte; outro começa do maior ao menor
    /// (métricas) ou de A a Z (Nome).
    fn click(self, col: Col) -> Self {
        if col == self.col {
            Self {
                descending: !self.descending,
                ..self
            }
        } else {
            Self {
                col,
                descending: col != Col::Name,
            }
        }
    }
}

fn running(c: &Container) -> bool {
    c.state == "running"
}

fn order(a: &Container, b: &Container, sort: Sort) -> Ordering {
    let by_name = || {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    };
    let directed = |o: Ordering| if sort.descending { o.reverse() } else { o };
    if sort.col == Col::Name {
        return directed(by_name());
    }
    match (sort.col.metric(a), sort.col.metric(b)) {
        (Some(x), Some(y)) => directed(x.total_cmp(&y)).then_with(by_name),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => running(b).cmp(&running(a)).then_with(by_name),
    }
}

/// A ordem padrão da tabela (CPU decrescente; quem não tem a métrica no
/// fim, por nome): a Visão geral lista os em execução nela.
pub(crate) fn default_order(a: &Container, b: &Container) -> Ordering {
    order(a, b, Sort::default())
}

/// Estado do Docker em pt-BR; um estado fora da lista do Docker vem como veio.
fn state_label(state: &str) -> &str {
    match state {
        "running" => "em execução",
        "exited" => "parado",
        "paused" => "pausado",
        "created" => "criado",
        "restarting" => "reiniciando",
        "removing" => "removendo",
        "dead" => "morto",
        "" => "—",
        other => other,
    }
}

fn dash(text: &str) -> String {
    if text.trim().is_empty() {
        "—".into()
    } else {
        text.to_owned()
    }
}

/// "1.2GB / 840MB" do `docker stats` em pt-BR, com o sentido de cada lado:
/// "1,2 GB ↓ · 840 MB ↑". Vazio ou "--" = "—"; fora do formato, como veio.
fn io(raw: &str, first: &str, second: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() || raw == "--" {
        return "—".into();
    }
    let amount = |side: &str| {
        let side = side.trim();
        let at = side.find(|c: char| !c.is_ascii_digit() && c != '.')?;
        let (n, unit) = side.split_at(at);
        let n: f64 = n.parse().ok()?;
        (!unit.is_empty() && unit.chars().all(|c| c.is_ascii_alphabetic()))
            .then(|| format!("{} {unit}", decimal_up_to(n, 3)))
    };
    match raw.split_once(" / ").map(|(a, b)| (amount(a), amount(b))) {
        Some((Some(a), Some(b))) => format!("{a} {first} · {b} {second}"),
        _ => raw.to_owned(),
    }
}

/// Uma publicação (IP do host, porta do host, destino) ou a entrada inteira,
/// quando não é publicação.
type PortEntry<'a> = Result<(Option<&'a str>, &'a str, &'a str), &'a str>;
/// As entradas de `Ports` do `docker ps` (separadas por vírgula): a porta do
/// host com o IP (`None` nos curingas 0.0.0.0, [::] e ::) e o destino de cada
/// publicação; o que não é publicação vem inteiro em `Err`.
fn port_entries(raw: &str) -> Vec<PortEntry<'_>> {
    raw.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|entry| {
            let Some((host, target)) = entry.split_once("->") else {
                return Err(entry);
            };
            let (ip, port) = host.rsplit_once(':').unwrap_or(("", host));
            let numeric = !port.is_empty() && port.chars().all(|c| c.is_ascii_digit() || c == '-');
            if !numeric || target.is_empty() {
                return Err(entry);
            }
            let ip = (!matches!(ip, "" | "0.0.0.0" | "::" | "[::]")).then_some(ip);
            Ok((ip, port, target))
        })
        .collect()
}
fn unique(items: impl Iterator<Item = String>) -> String {
    let mut seen: Vec<String> = Vec::new();
    for item in items {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    if seen.is_empty() {
        "—".into()
    } else {
        seen.join(", ")
    }
}
/// Portas na tabela: só as do host, sem repetir ("49374", e não a mesma em
/// IPv4 e IPv6); a que não é publicada vem como veio ("5432/tcp").
fn host_ports(raw: &str) -> String {
    unique(port_entries(raw).into_iter().map(|e| match e {
        Ok((_, port, _)) => port.to_owned(),
        Err(entry) => entry.to_owned(),
    }))
}
/// Portas no inspetor: "49374 → 49374/tcp" por publicação, sem repetir; um IP
/// que não é curinga fica ("127.0.0.1:8080 → 80/tcp").
fn port_mappings(raw: &str) -> String {
    unique(port_entries(raw).into_iter().map(|e| match e {
        Ok((Some(ip), port, target)) => format!("{ip}:{port} → {target}"),
        Ok((None, port, target)) => format!("{port} → {target}"),
        Err(entry) => entry.to_owned(),
    }))
}

/// Duração do `docker ps` ("3 days", "About an hour") em pt-BR.
fn duration(text: &str) -> Option<String> {
    match text {
        "Less than a second" => return Some("menos de um segundo".into()),
        "About a minute" => return Some("cerca de um minuto".into()),
        "About an hour" => return Some("cerca de uma hora".into()),
        _ => {}
    }
    let (n, unit) = text.split_once(' ')?;
    let n: usize = n.parse().ok()?;
    let (one, many) = match unit.trim_end_matches('s') {
        "second" => ("segundo", "segundos"),
        "minute" => ("minuto", "minutos"),
        "hour" => ("hora", "horas"),
        "day" => ("dia", "dias"),
        "week" => ("semana", "semanas"),
        "month" => ("mês", "meses"),
        "year" => ("ano", "anos"),
        _ => return None,
    };
    Some(format!("{} {}", number(n), if n == 1 { one } else { many }))
}
/// "(0) 2 hours ago" de Exited e Restarting: "há 2 horas (código 0)".
fn exited(rest: &str) -> Option<String> {
    let (code, rest) = rest.strip_prefix('(')?.split_once(") ")?;
    code.parse::<i64>().ok()?;
    let ago = duration(rest.strip_suffix(" ago")?)?;
    Some(format!("há {ago} (código {code})"))
}
/// O `Status` do `docker ps` em pt-BR ("Up 3 days" → "Em execução há 3
/// dias"). Vazio = o estado; um formato desconhecido vem como veio.
fn status_label(status: &str, state: &str) -> String {
    let status = status.trim();
    let known = match status {
        "" => Some(state_label(state).to_owned()),
        "Created" => Some("Criado".into()),
        "Dead" => Some("Morto".into()),
        "Removal In Progress" => Some("Removendo".into()),
        _ => None,
    };
    if let Some(known) = known {
        return known;
    }
    let translated = if let Some(up) = status.strip_prefix("Up ") {
        let (up, note) = match up.rsplit_once(" (") {
            Some((up, note)) => match note {
                "Paused)" => (up, Some("pausado")),
                "healthy)" => (up, Some("saudável")),
                "unhealthy)" => (up, Some("não saudável")),
                "health: starting)" => (up, Some("verificação de saúde em andamento")),
                _ => (up, Some("")),
            },
            None => (up, None),
        };
        match (duration(up), note) {
            (Some(d), None) => Some(format!("Em execução há {d}")),
            (Some(d), Some(note)) if !note.is_empty() => {
                Some(format!("Em execução há {d} · {note}"))
            }
            _ => None,
        }
    } else if let Some(rest) = status.strip_prefix("Exited ") {
        exited(rest).map(|t| format!("Parado {t}"))
    } else if let Some(rest) = status.strip_prefix("Restarting ") {
        exited(rest).map(|t| format!("Reiniciando {t}"))
    } else {
        None
    };
    translated.unwrap_or_else(|| status.to_owned())
}

/// O ID curto do Docker (12 caracteres).
fn short(id: &str) -> String {
    id.chars().take(12).collect()
}

pub(crate) struct Docker {
    tab: Tab,
    search: String,
    running_only: bool,
    sort: Sort,
    /// ID do contêiner no inspetor.
    selected: Option<String>,
}
impl Default for Docker {
    fn default() -> Self {
        Self {
            tab: Tab::Containers,
            search: String::new(),
            running_only: false,
            sort: Sort::default(),
            selected: None,
        }
    }
}

impl Docker {
    /// Reconexão: a seleção (os contêineres são de outra conexão). Aba,
    /// busca, "Só em execução" e ordenação ficam.
    pub(crate) fn reset(&mut self) {
        self.selected = None;
    }
    /// A Visão geral abre um contêiner: a aba Contêineres com ele no
    /// inspetor. A busca sai, para ele não ficar fora da lista.
    pub(crate) fn open(&mut self, id: &str) {
        self.tab = Tab::Containers;
        self.search.clear();
        self.selected = Some(id.to_owned());
    }
    #[cfg(test)]
    pub(crate) fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    #[cfg(test)]
    pub(crate) fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }
    #[cfg(test)]
    pub(crate) fn search_mut(&mut self) -> &mut String {
        &mut self.search
    }

    /// A página; `monitored` = o contêiner do ai-memory em Ajustes.
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        r: &ResourceSnapshot,
        monitored: &str,
        now: DateTime<Utc>,
    ) {
        self.toolbar(ui, r);
        let needle = self.search.to_lowercase();
        let page = [
            match &r.docker_context {
                Some(context) => format!("Contexto {context} · {UNITS}"),
                None => UNITS.to_owned(),
            },
            match r.images_at {
                Some(at) => format!("Imagens: inventário {}", ago((now - at).num_seconds())),
                None => "Imagens: aguardando o inventário".to_owned(),
            },
        ];
        match self.tab {
            Tab::Containers => {
                self.summary(ui, r);
                let matching: Vec<&Container> = r
                    .containers
                    .iter()
                    .filter(|c| {
                        format!("{} {} {}", c.name, c.image, c.id)
                            .to_lowercase()
                            .contains(&needle)
                    })
                    .collect();
                let hidden = if self.running_only {
                    matching.iter().filter(|c| !running(c)).count()
                } else {
                    0
                };
                let rows = matching
                    .into_iter()
                    .filter(|c| !self.running_only || running(c))
                    .collect();
                widgets::split(ui, &page, |ui| {
                    self.containers(ui, rows, hidden, r, monitored)
                });
            }
            Tab::Images => {
                if let Some(e) = &r.inventory_error {
                    widgets::strip(
                        ui,
                        Tone::Warn,
                        "Inventário de imagens não atualizado.",
                        e,
                        None,
                    );
                }
                let images: Vec<&DockerImage> = r
                    .images
                    .iter()
                    .filter(|i| {
                        format!("{} {}", i.tags.join(" "), i.id)
                            .to_lowercase()
                            .contains(&needle)
                    })
                    .collect();
                let mut legend = vec![IMAGES_NOTE.to_owned(), SIZE_NOTE.to_owned()];
                legend.extend(page);
                widgets::split(ui, &legend, |ui| images_table(ui, &images, r));
            }
            Tab::Processes => {
                let processes: Vec<&HostProcess> = r
                    .processes
                    .iter()
                    .filter(|p| process_matches(p, &needle))
                    .collect();
                let mut legend = vec![crate::platform::PROCESSES_CAPTION.to_owned()];
                legend.extend(page);
                widgets::split(ui, &legend, |ui| processes_table(ui, &processes, r));
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, r: &ResourceSnapshot) {
        let containers = if r.at.is_some() && r.docker_error.is_none() {
            number(r.containers.len())
        } else {
            "—".into()
        };
        let images = r
            .images_at
            .map_or_else(|| "—".into(), |_| number(r.images.len()));
        let processes = format!("Processos do {}", crate::platform::HOST);
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
                            value: Tab::Containers,
                            label: "Contêineres",
                            count: Some(containers),
                        },
                        Segment {
                            value: Tab::Images,
                            label: "Imagens",
                            count: Some(images),
                        },
                        Segment {
                            value: Tab::Processes,
                            label: &processes,
                            count: None,
                        },
                    ],
                );
            },
            |ui| {
                let width = ui.available_width().min(200.);
                if width >= 140. {
                    widgets::search_field(ui, search, width, SEARCH);
                } else {
                    below = true;
                }
            },
        );
        if below {
            let width = ui.available_width().min(320.);
            widgets::search_field(ui, &mut self.search, width, SEARCH);
        }
    }

    /// "3 em execução de 8", CPU, Memória e Tarefas, e "Só em execução" à
    /// direita: com o espaço justo se preciso e, se nem assim couber, embaixo.
    fn summary(&mut self, ui: &mut Ui, r: &ResourceSnapshot) {
        let p = theme::palette(ui.visuals());
        let total = r.docker_total();
        let unknown = || "—".to_owned();
        let count = if r.at.is_some() && r.docker_error.is_none() {
            let on = r.containers.iter().filter(|c| running(c)).count();
            (
                number(on),
                format!("em execução de {}", number(r.containers.len())),
            )
        } else {
            (unknown(), "em execução".to_owned())
        };
        let items: [(String, String, bool); 4] = [
            (count.0, count.1, true),
            (
                "CPU".into(),
                total
                    .as_ref()
                    .map_or_else(unknown, |u| format!("{}%", decimal(u.cpu, 1))),
                false,
            ),
            (
                "Memória".into(),
                total.as_ref().map_or_else(unknown, |u| bytes(u.memory)),
                false,
            ),
            (
                "Tarefas".into(),
                total
                    .as_ref()
                    .and_then(|u| u.tasks)
                    .map_or_else(unknown, |t| number(t as usize)),
                false,
            ),
        ];
        let galleys: Vec<_> = items
            .iter()
            .map(|(a, b, value_first)| {
                let strong = egui::TextFormat::simple(FontId::new(13., theme::semibold()), p.text);
                let soft = egui::TextFormat::simple(FontId::proportional(12.), p.text2);
                let mut job = egui::text::LayoutJob::default();
                let (first, second) = if *value_first {
                    (strong, soft)
                } else {
                    (soft, strong)
                };
                job.append(a, 0., first);
                job.append(&format!(" {b}"), 0., second);
                ui.painter().layout_job(job)
            })
            .collect();
        let switch = ui
            .painter()
            .layout_no_wrap(ONLY_RUNNING.into(), Type::Body.font(), p.text)
            .size()
            .x
            + 26.
            + 8.;
        let line_width = |gap: f32| {
            galleys.iter().map(|g| g.size().x).sum::<f32>() + gap * galleys.len() as f32 + switch
        };
        let width = ui.available_width();
        let fits = line_width(SUMMARY_TIGHT) <= width;
        let gap = if line_width(SUMMARY_GAP) <= width {
            SUMMARY_GAP
        } else {
            SUMMARY_TIGHT
        };
        let line = ui
            .horizontal(|ui| {
                ui.set_min_height(SUMMARY);
                ui.spacing_mut().item_spacing.x = gap;
                for (i, galley) in galleys.into_iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
                    let spoken = galley.text().to_owned();
                    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &spoken));
                    ui.painter().galley(rect.min, galley, p.text);
                    // O 4º item é Tarefas.
                    if i == 3 {
                        response.on_hover_text(TASKS_HINT);
                    }
                }
                if fits {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        widgets::switch(ui, &mut self.running_only, ONLY_RUNNING);
                    });
                }
            })
            .response
            .rect;
        let mut bottom = line.bottom();
        if !fits {
            bottom = ui
                .horizontal(|ui| {
                    ui.set_min_height(28.);
                    widgets::switch(ui, &mut self.running_only, ONLY_RUNNING);
                })
                .response
                .rect
                .bottom();
        }
        ui.painter().hline(
            ui.max_rect().x_range(),
            bottom + 0.5,
            Stroke::new(1.0_f32, p.line),
        );
    }

    /// A tabela (cabeçalho fixo, linhas visíveis), a linha dos ocultos e o
    /// inspetor, ao lado ou abaixo.
    fn containers(
        &mut self,
        ui: &mut Ui,
        mut rows: Vec<&Container>,
        hidden: usize,
        r: &ResourceSnapshot,
        monitored: &str,
    ) {
        let p = theme::palette(ui.visuals());
        let rect = ui.max_rect();
        let side = rect.width() >= SIDE_BY_SIDE && !rows.is_empty();
        let table = if side {
            Rect::from_min_max(rect.min, pos2(rect.right() - INSPECTOR - 1., rect.bottom()))
        } else {
            rect
        };
        widgets::pane(ui, table, "Contêineres");
        let columns = columns(table.width());
        self.header(ui, table, &columns);
        rows.sort_by(|a, b| order(a, b, self.sort));
        if !rows
            .iter()
            .any(|c| self.selected.as_deref() == Some(c.id.as_str()))
        {
            self.selected = rows
                .iter()
                .find(|c| c.name == monitored)
                .or(rows.first())
                .map(|c| c.id.clone());
        }
        let selected = self.selected.clone();
        let find = |id: &Option<String>| {
            rows.iter()
                .copied()
                .find(|c| id.as_deref() == Some(c.id.as_str()))
        };
        let body = Rect::from_min_max(pos2(table.left(), table.top() + HEAD), table.max);
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(body)
                .layout(Layout::top_down(Align::Min)),
        );
        let (mut clicked, mut show_all) = (None, false);
        egui::ScrollArea::vertical()
            .id_salt("docker-containers")
            .auto_shrink(false)
            .show_viewport(&mut child, |ui, viewport| {
                let origin = ui.max_rect().left_top();
                let width = ui.max_rect().width();
                let first = (viewport.min.y / ROW).floor().max(0.) as usize;
                let last = ((viewport.max.y / ROW).ceil().max(0.) as usize).min(rows.len());
                for (i, c) in rows.iter().enumerate().take(last).skip(first) {
                    let at = Rect::from_min_size(
                        pos2(origin.x, origin.y + i as f32 * ROW),
                        vec2(width, ROW),
                    );
                    let on = selected.as_deref() == Some(c.id.as_str());
                    if row(ui, at, c, &columns, on).clicked() {
                        clicked = Some(c.id.clone());
                    }
                }
                if rows.is_empty() {
                    let subtitle = if r.at.is_none() {
                        "Aguardando a primeira coleta do Docker."
                    } else if r.docker_error.is_some() {
                        "Docker indisponível. Veja o aviso acima."
                    } else if r.containers.is_empty() {
                        "Nenhum contêiner neste contexto Docker."
                    } else {
                        "Nenhum contêiner corresponde à busca ou ao filtro."
                    };
                    widgets::empty(ui, "Nenhum contêiner nesta lista", subtitle);
                } else {
                    ui.advance_cursor_after_rect(Rect::from_min_size(
                        origin,
                        vec2(width, ROW * rows.len() as f32),
                    ));
                }
                if hidden > 0 {
                    show_all = hidden_line(ui, hidden);
                }
                if !side && let Some(c) = find(&clicked.clone().or(selected.clone())) {
                    ui.add_space(space::S);
                    let shown = egui::Frame::new()
                        .fill(p.surf)
                        .inner_margin(egui::Margin::symmetric(20, 18))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            inspector(ui, c, r, monitored);
                        })
                        .response;
                    ui.painter().hline(
                        shown.rect.x_range(),
                        shown.rect.top() + 0.5,
                        Stroke::new(1.0_f32, p.line),
                    );
                    shown.widget_info(|| {
                        WidgetInfo::labeled(WidgetType::Panel, true, "Inspetor do contêiner")
                    });
                }
            });
        if side && let Some(c) = find(&clicked.clone().or(selected)) {
            let at = Rect::from_min_max(pos2(table.right() + 1., rect.top()), rect.max);
            widgets::pane(ui, at, "Inspetor do contêiner");
            ui.painter().rect_filled(at, 0., p.surf);
            ui.painter()
                .vline(at.left() - 0.5, at.y_range(), Stroke::new(1.0_f32, p.line));
            let mut column = ui.new_child(
                UiBuilder::new()
                    .max_rect(at)
                    .layout(Layout::top_down(Align::Min)),
            );
            egui::ScrollArea::vertical()
                .id_salt(("docker-inspector", &c.id))
                .auto_shrink(false)
                .show(&mut column, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(20, 18))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            inspector(ui, c, r, monitored);
                        });
                });
        }
        if clicked.is_some() {
            self.selected = clicked;
        }
        if show_all {
            self.running_only = false;
        }
    }

    /// Cabeçalho de `HEAD`: Nome, CPU, Memória e Tarefas ordenam ao clique,
    /// com a seta no ativo (600, text); a direção vai ao AccessKit.
    fn header(&mut self, ui: &mut Ui, rect: Rect, columns: &[(Col, f32)]) {
        let p = theme::palette(ui.visuals());
        let mut x = rect.left();
        let mut chosen = None;
        for &(col, width) in columns {
            let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(width, HEAD));
            x += width;
            let active = col == self.sort.col;
            let (title, font, color) = if active {
                let arrow = if self.sort.descending { "↓" } else { "↑" };
                (
                    format!("{} {arrow}", col.title()),
                    FontId::new(11., theme::semibold()),
                    p.text,
                )
            } else {
                (col.title().to_owned(), FontId::proportional(11.), p.text2)
            };
            let inner = cell.shrink2(vec2(PAD, 0.));
            let galley = widgets::one_line(ui, &title, font, color, inner.width());
            let left = if col == Col::Tasks {
                inner.right() - galley.size().x
            } else {
                inner.left()
            };
            let id = ui.id().with(("docker-head", col.title()));
            if col.sorts() {
                let mut response = ui.interact(cell, id, Sense::click());
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, col.title()));
                if col == Col::Tasks {
                    response = response.on_hover_text(TASKS_HINT);
                }
                if active {
                    let direction = if self.sort.descending {
                        SortDirection::Descending
                    } else {
                        SortDirection::Ascending
                    };
                    ui.ctx().accesskit_node_builder(response.id, |node| {
                        node.set_sort_direction(direction);
                    });
                }
                if response.clicked() {
                    chosen = Some(col);
                }
            } else {
                ui.interact(cell, id, Sense::hover())
                    .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, col.title()));
            }
            ui.painter().galley(
                pos2(left, cell.center().y - galley.size().y / 2.),
                galley,
                color,
            );
        }
        ui.painter().hline(
            rect.x_range(),
            rect.top() + HEAD - 0.5,
            Stroke::new(1.0_f32, p.line),
        );
        if let Some(col) = chosen {
            self.sort = self.sort.click(col);
        }
    }
}

/// As colunas para a largura da tabela: Nome com o resto; Portas só a partir
/// de `PORTS_FROM`.
fn columns(width: f32) -> Vec<(Col, f32)> {
    let ports = width >= PORTS_FROM;
    let fixed = CPU_WIDTH + MEMORY_WIDTH + TASKS_WIDTH + if ports { PORTS_WIDTH } else { 0. };
    let mut columns = vec![
        (Col::Name, (width - fixed).max(FLEX_MIN)),
        (Col::Cpu, CPU_WIDTH),
        (Col::Memory, MEMORY_WIDTH),
        (Col::Tasks, TASKS_WIDTH),
    ];
    if ports {
        columns.push((Col::Ports, PORTS_WIDTH));
    }
    columns
}

/// Uma linha de `ROW`: ponto, nome e "imagem · estado"; CPU e memória com a
/// barra; tarefas; portas. Selecionada: fundo sel e o filete acc de 3 px.
fn row(ui: &mut Ui, rect: Rect, c: &Container, columns: &[(Col, f32)], selected: bool) -> Response {
    let p = theme::palette(ui.visuals());
    let on = running(c);
    let usage = c.usage.as_ref();
    let unknown = || "—".to_owned();
    let cpu = usage.map_or_else(unknown, |u| format!("{}%", decimal(u.cpu, 1)));
    let memory = usage.map_or_else(unknown, |u| bytes(u.memory));
    let tasks = usage
        .and_then(|u| u.tasks)
        .map_or_else(unknown, |t| number(t as usize));
    let ports = host_ports(&c.ports);
    let state = state_label(&c.state);
    let second = format!("{} · {state}", dash(&c.image));
    let response = ui.interact(rect, ui.id().with(("container", &c.id)), Sense::click());
    let spoken = format!(
        "{} · {state} · CPU {cpu} · Memória {memory} · Tarefas {tasks}",
        c.name
    );
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, &spoken));
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
    let value_color = if usage.is_some() { p.text } else { p.text2 };
    let mut elided = false;
    let mut x = rect.left();
    for &(col, width) in columns {
        let cell =
            Rect::from_min_size(pos2(x, rect.top()), vec2(width, ROW)).shrink2(vec2(PAD, 0.));
        x += width;
        match col {
            Col::Name => {
                ui.painter().circle_filled(
                    pos2(cell.left() + 3.5, rect.center().y),
                    3.5,
                    if on { p.acc } else { p.text2 },
                );
                let left = cell.left() + 7. + 10.;
                let name = widgets::one_line(
                    ui,
                    &c.name,
                    Type::Body.font(),
                    if on { p.text } else { p.text2 },
                    cell.right() - left,
                );
                let below = widgets::one_line(
                    ui,
                    &second,
                    FontId::proportional(11.),
                    p.text2,
                    cell.right() - left,
                );
                elided |= name.elided || below.elided;
                let top = rect.center().y - (name.size().y + below.size().y) / 2.;
                let name_height = name.size().y;
                ui.painter().galley(pos2(left, top), name, p.text);
                ui.painter()
                    .galley(pos2(left, top + name_height), below, p.text2);
            }
            Col::Cpu => bar_cell(
                ui,
                cell,
                &cpu,
                value_color,
                CPU_VALUE,
                usage.map(|u| (u.cpu / 100.) as f32),
                p.acc,
                &format!("CPU de {}", c.name),
            ),
            Col::Memory => bar_cell(
                ui,
                cell,
                &memory,
                value_color,
                MEMORY_VALUE,
                usage
                    .filter(|u| u.limit > 0)
                    .map(|u| (u.memory as f64 / u.limit as f64) as f32),
                p.series2,
                &format!("Memória de {}", c.name),
            ),
            Col::Tasks => {
                let galley =
                    widgets::one_line(ui, &tasks, Type::Body.font(), p.text2, cell.width());
                ui.painter().galley(
                    pos2(
                        cell.right() - galley.size().x,
                        rect.center().y - galley.size().y / 2.,
                    ),
                    galley,
                    p.text2,
                );
            }
            Col::Ports => {
                let galley =
                    widgets::one_line(ui, &ports, Type::Mono.font(), p.text2, cell.width());
                elided |= galley.elided;
                ui.painter().galley(
                    pos2(cell.left(), rect.center().y - galley.size().y / 2.),
                    galley,
                    p.text2,
                );
            }
        }
    }
    if elided {
        response.on_hover_text(format!("{}\n{second}\n{ports}", c.name))
    } else {
        response
    }
}

/// Valor alinhado à direita em `slot` (ou mais, se não couber) e a barra
/// `THIN` no resto da célula.
#[allow(clippy::too_many_arguments)]
fn bar_cell(
    ui: &mut Ui,
    cell: Rect,
    value: &str,
    color: Color32,
    slot: f32,
    fraction: Option<f32>,
    bar: Color32,
    label: &str,
) {
    let galley = ui
        .painter()
        .layout_no_wrap(value.to_owned(), Type::Body.font(), color);
    let right = cell.left() + slot.max(galley.size().x);
    ui.painter().galley(
        pos2(
            right - galley.size().x,
            cell.center().y - galley.size().y / 2.,
        ),
        galley,
        color,
    );
    let track = Rect::from_min_max(
        pos2(right + 8., cell.center().y - THIN / 2.),
        pos2(cell.right(), cell.center().y + THIN / 2.),
    );
    if track.width() > 0. {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(track)
                .layout(Layout::top_down(Align::Min)),
        );
        widgets::meter(&mut child, fraction, THIN, bar, label);
    }
}

/// "N contêineres parados ocultos · mostrar"; `true` no link.
fn hidden_line(ui: &mut Ui, hidden: usize) -> bool {
    let p = theme::palette(ui.visuals());
    let text = if hidden == 1 {
        "1 contêiner parado oculto".to_owned()
    } else {
        format!("{} contêineres parados ocultos", number(hidden))
    };
    ui.horizontal(|ui| {
        ui.set_min_height(36.);
        ui.add_space(PAD);
        ui.spacing_mut().item_spacing.x = 4.;
        // Com seleção de texto, o egui anuncia o link como Label.
        ui.style_mut().interaction.selectable_labels = false;
        ui.label(RichText::new(text).size(12.).color(p.text2));
        ui.label(RichText::new("·").size(12.).color(p.text2));
        ui.add(egui::Link::new(
            RichText::new("mostrar").size(12.).color(p.acc),
        ))
        .clicked()
    })
    .inner
}

/// O conteúdo do inspetor: ponto e nome, o status do Docker, os fatos, os
/// processos do contêiner monitorado e os botões de copiar.
fn inspector(ui: &mut Ui, c: &Container, r: &ResourceSnapshot, monitored: &str) {
    let p = theme::palette(ui.visuals());
    let on = running(c);
    let ours = c.name == monitored;
    ui.spacing_mut().item_spacing.y = 4.;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.;
        let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.), Sense::hover());
        ui.painter()
            .circle_filled(dot.center(), 4., if on { p.acc } else { p.text2 });
        ui.add(Label::new(RichText::new(&c.name).font(Type::Bar.font())).truncate());
    });
    let mut status = status_label(&c.status, &c.state);
    if ours {
        status.push_str(" · contêiner monitorado");
    }
    ui.add(Label::new(RichText::new(status).size(12.).color(p.text2)).truncate());
    ui.add_space(10.);
    let limit = c
        .usage
        .as_ref()
        .filter(|u| u.limit > 0)
        .map_or_else(|| "—".to_owned(), |u| bytes(u.limit));
    widgets::facts(
        ui,
        &[
            Fact::new("Imagem", dash(&c.image)),
            Fact::new("Portas", port_mappings(&c.ports)).mono(),
            Fact::new("Limite de memória", limit),
            Fact::new("Rede acumulada", io(&c.net_io, "↓", "↑")),
            Fact::new("Disco acumulado", io(&c.block_io, "lidos", "gravados")),
            Fact::new("ID", dash(&short(&c.id))).mono(),
        ],
    );
    if ours {
        ui.add_space(10.);
        ui.label(
            RichText::new("Processos no contêiner")
                .size(12.)
                .family(theme::semibold())
                .color(p.text2),
        );
        if let Some(e) = &r.ai_process_error {
            widgets::caption(ui, e);
        } else if !on {
            widgets::caption(ui, "Disponível com o contêiner em execução.");
        } else if r.ai_processes.is_empty() {
            widgets::caption(ui, "—");
        } else {
            processes_in_container(ui, &r.ai_processes);
        }
    }
    ui.add_space(10.);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S;
        if ui.add(Button::secondary("Copiar ID")).clicked() {
            ui.ctx().copy_text(c.id.clone());
        }
        if ui.add(Button::secondary("Copiar nome")).clicked() {
            ui.ctx().copy_text(c.name.clone());
        }
    });
}

/// PID (text2), Processo e Threads (à direita), em linhas de `INSPECTOR_ROW`.
fn processes_in_container(ui: &mut Ui, rows: &[(String, String, u64)]) {
    let p = theme::palette(ui.visuals());
    let widths = |width: f32| [56., (width - 56. - 64.).max(40.), 64.];
    let line = |ui: &mut Ui, cells: [(String, Color32, f32); 3]| {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), INSPECTOR_ROW), Sense::hover());
        let mut x = rect.left();
        for (i, ((text, color, size), width)) in
            cells.into_iter().zip(widths(rect.width())).enumerate()
        {
            let at = Rect::from_min_size(pos2(x, rect.top()), vec2(width, INSPECTOR_ROW));
            x += width;
            let layout = if i == 2 {
                Layout::right_to_left(Align::Center)
            } else {
                Layout::left_to_right(Align::Center)
            };
            ui.new_child(UiBuilder::new().id_salt(i).max_rect(at).layout(layout))
                .add(Label::new(RichText::new(text).size(size).color(color)).truncate());
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.line),
        );
    };
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.;
        line(
            ui,
            [
                ("PID".into(), p.text2, 11.),
                ("Processo".into(), p.text2, 11.),
                ("Threads".into(), p.text2, 11.),
            ],
        );
        for (pid, name, threads) in rows {
            line(
                ui,
                [
                    (pid.clone(), p.text2, 12.),
                    (name.clone(), p.text, 12.),
                    (number(*threads as usize), p.text, 12.),
                ],
            );
        }
    });
}

const IMAGE_COLUMNS: [widgets::Column; 4] = [
    widgets::Column {
        title: "Imagem / tags",
        width: None,
        hint: Some(IMAGES_HINT),
    },
    widgets::Column {
        title: "Tamanho",
        width: Some(100.),
        hint: None,
    },
    widgets::Column {
        title: "Uso",
        width: Some(130.),
        hint: None,
    },
    widgets::Column {
        title: "Contêineres",
        width: Some(200.),
        hint: None,
    },
];
const PROCESS_COLUMNS: [widgets::Column; 5] = [
    widgets::Column {
        title: "PID",
        width: Some(80.),
        hint: None,
    },
    widgets::Column {
        title: "Processo",
        width: None,
        hint: None,
    },
    widgets::Column {
        title: "CPU",
        width: Some(90.),
        hint: None,
    },
    widgets::Column {
        title: "Memória RSS",
        width: Some(110.),
        hint: None,
    },
    widgets::Column {
        title: "Threads",
        width: Some(90.),
        hint: None,
    },
];

/// Tabela vazia: a mensagem embaixo do cabeçalho.
fn empty_table(ui: &mut Ui, title: &str, subtitle: &str) {
    let rect = ui.max_rect();
    let below = Rect::from_min_max(pos2(rect.left(), rect.top() + HEAD), rect.max);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(below)
            .layout(Layout::top_down(Align::Min)),
    );
    widgets::empty(&mut child, title, subtitle);
}

fn cell_text(ui: &mut Ui, text: &str, color: Color32) -> Response {
    ui.add(Label::new(RichText::new(text).size(13.).color(color)).truncate())
}

fn images_table(ui: &mut Ui, images: &[&DockerImage], r: &ResourceSnapshot) {
    let p = theme::palette(ui.visuals());
    widgets::table(
        ui,
        "docker-images",
        &IMAGE_COLUMNS,
        widgets::TableSize {
            head: HEAD,
            row: ROW,
            pad: PAD,
        },
        images.len(),
        false,
        |ui, row, column| {
            let i = images[row];
            match column {
                0 => {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.;
                        ui.add_space(5.);
                        cell_text(
                            ui,
                            i.tags.first().map_or("<sem tag>", String::as_str),
                            p.text,
                        )
                        .on_hover_text(i.tags.join("\n"));
                        ui.add(
                            Label::new(
                                RichText::new(short_id(&i.id))
                                    .font(Type::Mono.font())
                                    .color(p.text2),
                            )
                            .truncate(),
                        );
                    });
                }
                1 => {
                    cell_text(ui, &bytes(i.size), p.text);
                }
                2 => {
                    let (text, color) = if i.running > 0 {
                        ("Em execução", p.acc)
                    } else if !i.containers.is_empty() {
                        ("Associada", p.text2)
                    } else {
                        ("Sem vínculo", p.text2)
                    };
                    ui.spacing_mut().item_spacing.x = 8.;
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(7.), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.5, color);
                    cell_text(ui, text, color);
                }
                _ => {
                    cell_text(
                        ui,
                        &if i.containers.is_empty() {
                            "—".to_owned()
                        } else {
                            i.containers.join(", ")
                        },
                        p.text2,
                    );
                }
            }
        },
    );
    if images.is_empty() {
        let subtitle = if r.docker_error.is_some() {
            "Docker indisponível. Veja o aviso acima."
        } else if r.images_at.is_none() && r.inventory_error.is_some() {
            "Inventário indisponível. Veja o aviso acima."
        } else if r.images_at.is_none() {
            "Aguardando o inventário de imagens."
        } else if r.images.is_empty() {
            "Nenhuma imagem local neste contexto Docker."
        } else {
            "Nenhuma imagem corresponde à busca."
        };
        empty_table(ui, "Nenhuma imagem nesta lista", subtitle);
    }
}

fn processes_table(ui: &mut Ui, processes: &[&HostProcess], r: &ResourceSnapshot) {
    let p = theme::palette(ui.visuals());
    widgets::table(
        ui,
        "docker-processes",
        &PROCESS_COLUMNS,
        widgets::TableSize {
            head: HEAD,
            row: PROCESS_ROW,
            pad: PAD,
        },
        processes.len(),
        false,
        |ui, row, column| {
            let process = processes[row];
            match column {
                0 => {
                    cell_text(ui, &process.pid.to_string(), p.text2);
                }
                1 => {
                    cell_text(ui, &process.name, p.text).on_hover_text(&process.group);
                }
                2 => {
                    cell_text(ui, &format!("{}%", decimal(process.usage.cpu, 1)), p.text);
                }
                3 => {
                    cell_text(ui, &bytes(process.usage.memory), p.text);
                }
                _ => {
                    cell_text(
                        ui,
                        &process
                            .usage
                            .tasks
                            .map_or_else(|| "—".to_owned(), |t| number(t as usize)),
                        p.text,
                    );
                }
            }
        },
    );
    if processes.is_empty() {
        let subtitle = if r.at.is_none() {
            "Aguardando a primeira coleta.".to_owned()
        } else if r.processes.is_empty() {
            format!(
                "Nenhum processo do Docker ou do ai-memory neste {}.",
                crate::platform::HOST
            )
        } else {
            "Nenhum processo corresponde à busca.".to_owned()
        };
        empty_table(ui, "Nenhum processo nesta lista", &subtitle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Usage;
    use crate::widgets::tests::{
        THEMES, click_frames, context, node, nodes, rect_of, rects, run_sized, texts,
    };
    use egui::{
        Event, Pos2,
        accesskit::{Role, Toggled},
    };

    /// 1100 − 16 de margem = 1084 de conteúdo: inspetor ao lado.
    const WIDE: [f32; 2] = [1100., 800.];
    /// 610 − 16 = 594, o conteúdo de 720 no macOS: inspetor abaixo.
    const NARROW: [f32; 2] = [610., 600.];
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    const LONG_ID: &str = "8f3c21a9d0e4b7c6a5f4e3d2c1b0a9f8e7d6c5b4a3f2e1d0c9b8a7f6e5d4c3b2";

    fn running(name: &str, cpu: f64, mib: u64, tasks: u64, ports: &str) -> Container {
        Container {
            id: format!("{name}-{LONG_ID}"),
            name: name.into(),
            image: format!("{name}:latest"),
            state: "running".into(),
            status: "Up 3 days".into(),
            ports: ports.into(),
            usage: Some(Usage {
                cpu,
                memory: mib * MIB,
                limit: 8 * GIB,
                tasks: Some(tasks),
            }),
            net_io: "1.2GB / 840MB".into(),
            block_io: "3.4GB / 9.1GB".into(),
            ..Default::default()
        }
    }
    fn stopped(name: &str, state: &str) -> Container {
        Container {
            id: format!("{name}-{LONG_ID}"),
            name: name.into(),
            image: format!("{name}:latest"),
            state: state.into(),
            status: "Exited (0) 2 hours ago".into(),
            ..Default::default()
        }
    }
    /// Os oito do artboard: três em execução (15,5% · 733 MiB · 64 tarefas).
    fn eight() -> Vec<Container> {
        vec![
            running(
                "ai-memory",
                12.4,
                421,
                46,
                "0.0.0.0:49374->49374/tcp, [::]:49374->49374/tcp",
            ),
            running("postgres-dev", 2.7, 218, 12, "0.0.0.0:5432->5432/tcp"),
            running("redis-cache", 0.4, 94, 6, "6379/tcp"),
            stopped("minio", "exited"),
            stopped("grafana", "paused"),
            stopped("loki", "created"),
            stopped("tempo", "exited"),
            stopped("zipkin", "dead"),
        ]
    }
    fn collected(containers: Vec<Container>) -> ResourceSnapshot {
        ResourceSnapshot {
            at: Some(Utc::now()),
            containers,
            ..Default::default()
        }
    }

    /// A página num contexto, com os dados e o relógio fixos.
    struct Page {
        ctx: egui::Context,
        size: [f32; 2],
        docker: Docker,
        r: ResourceSnapshot,
        monitored: String,
        now: DateTime<Utc>,
    }
    impl Page {
        fn new(theme: egui::Theme, size: [f32; 2], r: ResourceSnapshot) -> Self {
            Self {
                ctx: context(theme),
                size,
                docker: Docker::default(),
                r,
                monitored: "ai-memory".into(),
                now: Utc::now(),
            }
        }
        fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
            let Self {
                ctx,
                size,
                docker,
                r,
                monitored,
                now,
            } = self;
            run_sized(ctx, *size, events, |ui| docker.show(ui, r, monitored, *now))
        }
        fn show(&mut self) -> egui::FullOutput {
            self.frame(vec![])
        }
        /// Clique completo (`click_frames`); devolve o frame em que ele
        /// acontece (o de soltar).
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
        fn click_row(&mut self, name: &str) -> egui::FullOutput {
            let out = self.show();
            self.click(row_rect(&out, name).center())
        }
        /// Repousa o ponteiro além do atraso do tooltip (1/60 s por frame).
        fn rest(&mut self, pos: Pos2) -> egui::FullOutput {
            let mut out = self.frame(vec![Event::PointerMoved(pos)]);
            for _ in 0..45 {
                out = self.show();
            }
            out
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
    fn all(out: &egui::FullOutput) -> Vec<egui::accesskit::Node> {
        out.platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .map(|(_, n)| n.clone())
            .collect()
    }
    /// Linhas da tabela, de cima para baixo: (nome, selecionada).
    fn rows(out: &egui::FullOutput) -> Vec<(String, bool)> {
        let mut found: Vec<(f64, String, bool)> = all(out)
            .into_iter()
            .filter(|n| {
                n.role() == Role::Button && n.label().is_some_and(|l| l.contains(" · CPU "))
            })
            .map(|n| {
                (
                    n.bounds().unwrap().y0,
                    n.label().unwrap().split(" · ").next().unwrap().to_owned(),
                    n.toggled() == Some(Toggled::True),
                )
            })
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|(_, l, s)| (l, s)).collect()
    }
    fn names(out: &egui::FullOutput) -> Vec<String> {
        rows(out).into_iter().map(|(n, _)| n).collect()
    }
    fn selected(out: &egui::FullOutput) -> Vec<String> {
        rows(out)
            .into_iter()
            .filter(|(_, s)| *s)
            .map(|(n, _)| n)
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
    fn row_rect(out: &egui::FullOutput, name: &str) -> Rect {
        let found: Vec<Rect> = all(out)
            .iter()
            .filter(|n| {
                n.role() == Role::Button
                    && n.label()
                        .is_some_and(|l| l.starts_with(&format!("{name} · ")))
            })
            .map(rect_of)
            .collect();
        assert_eq!(found.len(), 1, "row {name}");
        found[0]
    }

    #[test]
    fn segment_counts_for_every_collection_state() {
        let now = Utc::now();
        let host = format!("Processos do {}", crate::platform::HOST);
        let images = |n: usize| vec![DockerImage::default(); n];
        // (caso, coleta, [Contêineres, Imagens])
        for (case, r, want) in [
            ("no collection", ResourceSnapshot::default(), ["—", "—"]),
            (
                "docker down",
                ResourceSnapshot {
                    at: Some(now),
                    docker_error: Some("Docker CLI não encontrado.".into()),
                    ..Default::default()
                },
                ["—", "—"],
            ),
            (
                "containers, no inventory yet",
                collected(eight()),
                ["8", "—"],
            ),
            (
                "with inventory",
                ResourceSnapshot {
                    images: images(1234),
                    images_at: Some(now),
                    ..collected(eight())
                },
                ["8", "1.234"],
            ),
            (
                "collected and empty",
                ResourceSnapshot {
                    images_at: Some(now),
                    ..collected(vec![])
                },
                ["0", "0"],
            ),
        ] {
            let mut page = Page::new(egui::Theme::Dark, WIDE, r);
            let out = page.show();
            let toggled = |text: &str| node(&out, Role::Button, text).toggled();
            assert_eq!(
                toggled(&format!("Contêineres {}", want[0])),
                Some(Toggled::True),
                "{case}: opens on Contêineres"
            );
            assert_eq!(
                toggled(&format!("Imagens {}", want[1])),
                Some(Toggled::False),
                "{case}"
            );
            assert_eq!(toggled(&host), Some(Toggled::False), "{case}");
        }
    }

    #[test]
    fn each_tab_shows_its_own_content_in_both_themes() {
        let host = format!("Processos do {}", crate::platform::HOST);
        for (theme, _) in THEMES {
            let r = ResourceSnapshot {
                images_at: Some(Utc::now()),
                docker_context: Some("desktop-linux".into()),
                ..collected(eight())
            };
            let mut page = Page::new(theme, WIDE, r);
            for (segment, tab) in [
                ("Imagens 0", Tab::Images),
                (host.as_str(), Tab::Processes),
                ("Contêineres 8", Tab::Containers),
            ] {
                page.click_on(Role::Button, segment);
                let out = page.show();
                assert_eq!(page.docker.tab, tab, "{theme:?}");
                assert_eq!(
                    node(&out, Role::Button, segment).toggled(),
                    Some(Toggled::True),
                    "{theme:?} {segment}"
                );
                node(
                    &out,
                    Role::Label,
                    "Contexto desktop-linux · CPU: 100% = 1 núcleo · memória sem cache inativo",
                );
                let found = (
                    has(&out, Role::CheckBox, ONLY_RUNNING),
                    has(&out, Role::Label, "3 em execução de 8"),
                    has(&out, Role::Pane, "Contêineres"),
                    has(&out, Role::Label, "Imagem / tags"),
                    has(&out, Role::Label, IMAGES_NOTE),
                    has(&out, Role::Label, "Memória RSS"),
                    has(&out, Role::Label, crate::platform::PROCESSES_CAPTION),
                );
                let want = match tab {
                    Tab::Containers => (true, true, true, false, false, false, false),
                    Tab::Images => (false, false, false, true, true, false, false),
                    Tab::Processes => (false, false, false, false, false, true, true),
                };
                assert_eq!(found, want, "{theme:?} {tab:?}");
            }
        }
    }

    #[test]
    fn summary_sums_the_running_containers_and_never_turns_unknown_into_zero() {
        let now = Utc::now();
        let mut no_stats = eight();
        no_stats[1].usage = None;
        let mut no_tasks = eight();
        no_tasks[2].usage.as_mut().unwrap().tasks = None;
        let unknown = ["— em execução", "CPU —", "Memória —", "Tarefas —"];
        // (caso, coleta, os quatro itens)
        for (case, r, want) in [
            ("no collection", ResourceSnapshot::default(), unknown),
            (
                "docker down",
                ResourceSnapshot {
                    at: Some(now),
                    docker_error: Some("falhou".into()),
                    ..Default::default()
                },
                unknown,
            ),
            (
                "all measured",
                collected(eight()),
                [
                    "3 em execução de 8",
                    "CPU 15,5%",
                    "Memória 733,0 MiB",
                    "Tarefas 64",
                ],
            ),
            (
                "a running one without stats",
                collected(no_stats),
                ["3 em execução de 8", "CPU —", "Memória —", "Tarefas —"],
            ),
            (
                "a running one without tasks",
                collected(no_tasks),
                [
                    "3 em execução de 8",
                    "CPU 15,5%",
                    "Memória 733,0 MiB",
                    "Tarefas —",
                ],
            ),
            (
                "nothing running",
                collected(vec![stopped("a", "exited"), stopped("b", "paused")]),
                ["0 em execução de 2", "CPU 0,0%", "Memória 0 B", "Tarefas 0"],
            ),
            (
                "collected, no containers",
                collected(vec![]),
                ["0 em execução de 0", "CPU 0,0%", "Memória 0 B", "Tarefas 0"],
            ),
        ] {
            let mut page = Page::new(egui::Theme::Light, WIDE, r);
            let out = page.show();
            let items: Vec<Rect> = want
                .iter()
                .map(|text| rect_of(&node(&out, Role::Label, text)))
                .collect();
            assert!(
                items.windows(2).all(|w| w[0].right() < w[1].left()
                    && (w[0].center().y - w[1].center().y).abs() < 4.),
                "{case}: one line, in order: {items:?}"
            );
            if want == unknown {
                assert!(
                    !has(&out, Role::Label, "CPU 0,0%"),
                    "{case}: unknown is not 0"
                );
            }
        }
    }

    #[test]
    fn summary_switch_tightens_the_gap_then_drops_below_only_when_it_must() {
        let mut big = eight();
        big[0].usage.as_mut().unwrap().cpu = 1234.5;
        big[0].usage.as_mut().unwrap().tasks = Some(123_456);
        // (largura, mesma linha?, espaço entre os itens): a linha toda pede
        // ~641 com 22 e ~610 com 14 (conteúdo = largura − 16).
        for (width, one_line, gap) in [
            (1100., true, SUMMARY_GAP),
            (640., true, SUMMARY_TIGHT),
            (500., false, SUMMARY_TIGHT),
        ] {
            let mut page = Page::new(egui::Theme::Dark, [width, 600.], collected(big.clone()));
            let out = page.show();
            let items: Vec<Rect> = ["3 em execução de 8", "CPU 1.237,6%", "Tarefas 123.474"]
                .iter()
                .map(|text| rect_of(&node(&out, Role::Label, text)))
                .collect();
            let memory = rect_of(&node(&out, Role::Label, "Memória 733,0 MiB"));
            assert_eq!(memory.left() - items[1].right(), gap, "{width}");
            let switch = rect_of(&node(&out, Role::CheckBox, ONLY_RUNNING));
            assert!(switch.right() <= width - 8. + 0.5, "{width}: {switch:?}");
            assert!(
                items.iter().all(|r| !r.intersects(switch)),
                "{width}: no overlap"
            );
            assert_eq!(
                switch.top() > items[2].bottom(),
                !one_line,
                "{width}: {switch:?} / {:?}",
                items[2]
            );
            let table = rect_of(&node(&out, Role::Pane, "Contêineres"));
            assert!(
                table.top() > switch.bottom(),
                "{width}: the table starts below"
            );
        }
    }

    #[test]
    fn rows_paint_dot_name_state_values_and_bars_in_both_themes() {
        for (theme, p) in THEMES {
            let mut web = running("web", 25., 256, 7, "0.0.0.0:80->80/tcp");
            web.usage.as_mut().unwrap().limit = GIB;
            let mut busy = running("busy", 250., 64, 3, "");
            busy.usage.as_mut().unwrap().limit = 0;
            let r = collected(vec![web, busy, stopped("db", "exited")]);
            let mut page = Page::new(theme, NARROW, r);
            page.monitored = "web".into();
            let out = page.show();
            assert_eq!(
                rows(&out),
                [
                    ("busy".to_owned(), false),
                    ("web".to_owned(), true),
                    ("db".to_owned(), false),
                ]
            );
            let painted = texts(&out);
            for (text, color) in [
                ("web", p.text),
                ("db", p.text2),
                ("web:latest · em execução", p.text2),
                ("db:latest · parado", p.text2),
                ("25,0%", p.text),
                ("256,0 MiB", p.text),
                ("250,0%", p.text),
                ("7", p.text2),
                ("—", p.text2),
            ] {
                assert!(
                    painted.contains(&(text.to_owned(), color)),
                    "{theme:?}: {text} in {color:?}"
                );
            }
            // CPU: 100% = barra cheia; acima, cheia. Memória: uso ÷ limite;
            // sem limite ou sem métrica, só o trilho.
            let meter = |label: &str| node(&out, Role::ProgressIndicator, label);
            for (label, want) in [
                ("CPU de web", Some(25.)),
                ("Memória de web", Some(25.)),
                ("CPU de busy", Some(100.)),
                ("Memória de busy", None),
                ("CPU de db", None),
                ("Memória de db", None),
            ] {
                assert_eq!(meter(label).numeric_value(), want, "{theme:?}: {label}");
            }
            let painted = rects(&out);
            for (label, color, fraction) in [
                ("CPU de web", p.acc, 0.25),
                ("Memória de web", p.series2, 0.25),
                ("CPU de busy", p.acc, 1.),
            ] {
                let track = rect_of(&meter(label));
                assert_eq!(track.height(), THIN, "{label}");
                assert!(
                    painted.iter().any(|r| r.fill == color
                        && r.rect.min == track.min
                        && (r.rect.width() - track.width() * fraction).abs() < 0.01),
                    "{theme:?}: {label} fill"
                );
            }
            // Ponto acc em execução, text2 parado; linhas de 44 px.
            let (web, db) = (row_rect(&out, "web"), row_rect(&out, "db"));
            let dots: Vec<(f32, Color32)> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Circle(c) if c.radius == 3.5 => Some((c.center.y, c.fill)),
                    _ => None,
                })
                .collect();
            for (row, color) in [(web, p.acc), (db, p.text2)] {
                assert!(
                    dots.iter()
                        .any(|&(y, fill)| y == row.center().y && fill == color),
                    "{theme:?}: dot of {row:?}"
                );
                assert_eq!(row.height(), ROW);
            }
            assert_eq!(db.top() - web.top(), ROW);
            // Selecionada: fundo sel e o filete acc de 3 px; as outras não.
            assert!(painted.iter().any(|r| r.rect == web && r.fill == p.sel));
            assert!(painted.iter().any(|r| r.fill == p.acc
                && r.rect == Rect::from_min_size(web.min, vec2(3., web.height()))));
            assert!(!painted.iter().any(|r| r.rect == db && r.fill == p.sel));
        }
    }

    #[test]
    fn ports_are_mono_text2_and_appear_only_from_the_ports_width() {
        let r = collected(vec![running(
            "web",
            1.,
            1,
            1,
            "0.0.0.0:80->80/tcp, [::]:80->80/tcp",
        )]);
        for (size, shown) in [(NARROW, false), (WIDE, true), ([1300., 800.], true)] {
            let mut page = Page::new(egui::Theme::Dark, size, r.clone());
            let out = page.show();
            let row = row_rect(&out, "web");
            let ports = out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Text(t)
                    if t.galley.text() == "80" && row.contains_rect(t.visual_bounding_rect()) =>
                {
                    Some(t.clone())
                }
                _ => None,
            });
            assert_eq!(has(&out, Role::Label, "Portas"), shown, "{size:?}");
            assert_eq!(ports.is_some(), shown, "{size:?}: the column");
            if let Some(t) = ports {
                assert_eq!(t.galley.job.sections[0].format.font_id, Type::Mono.font());
                assert_eq!(t.galley.job.sections[0].format.color, theme::DARK.text2);
            }
            // No inspetor, sempre, por publicação.
            node(&out, Role::Label, "Portas: 80 → 80/tcp");
        }
        assert_eq!(PORTS_FROM, 710.);
        assert_eq!(columns(PORTS_FROM - 0.1).len(), 4);
        assert_eq!(columns(PORTS_FROM).len(), 5);
    }

    #[test]
    fn ports_show_host_ports_in_the_table_and_each_mapping_in_the_inspector() {
        // (Ports do `docker ps`, tabela, inspetor)
        for (raw, table, inspector) in [
            (
                "0.0.0.0:49374->49374/tcp, [::]:49374->49374/tcp",
                "49374",
                "49374 → 49374/tcp",
            ),
            ("0.0.0.0:5432->5432/tcp", "5432", "5432 → 5432/tcp"),
            ("6379/tcp", "6379/tcp", "6379/tcp"),
            (
                "0.0.0.0:80->80/tcp, 0.0.0.0:443->443/tcp, :::80->80/tcp, :::443->443/tcp",
                "80, 443",
                "80 → 80/tcp, 443 → 443/tcp",
            ),
            ("127.0.0.1:8080->80/tcp", "8080", "127.0.0.1:8080 → 80/tcp"),
            ("[::1]:8080->80/tcp", "8080", "[::1]:8080 → 80/tcp"),
            (
                "0.0.0.0:8000-8002->8000-8002/tcp",
                "8000-8002",
                "8000-8002 → 8000-8002/tcp",
            ),
            (
                "0.0.0.0:53->53/tcp, 0.0.0.0:53->53/udp",
                "53",
                "53 → 53/tcp, 53 → 53/udp",
            ),
            (
                "0.0.0.0:80->80/tcp, 443/tcp",
                "80, 443/tcp",
                "80 → 80/tcp, 443/tcp",
            ),
            ("", "—", "—"),
            ("  ", "—", "—"),
            (", ", "—", "—"),
            ("algo estranho", "algo estranho", "algo estranho"),
            ("0.0.0.0:->80/tcp", "0.0.0.0:->80/tcp", "0.0.0.0:->80/tcp"),
            ("0.0.0.0:80->", "0.0.0.0:80->", "0.0.0.0:80->"),
            (
                "0.0.0.0:http->80/tcp",
                "0.0.0.0:http->80/tcp",
                "0.0.0.0:http->80/tcp",
            ),
        ] {
            assert_eq!(
                (host_ports(raw).as_str(), port_mappings(raw).as_str()),
                (table, inspector),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn status_in_pt_br_for_every_docker_format_and_raw_when_unknown() {
        // (Status, estado, texto)
        for (status, state, want) in [
            ("Up 3 days", "running", "Em execução há 3 dias"),
            (
                "Up 5 hours (healthy)",
                "running",
                "Em execução há 5 horas · saudável",
            ),
            (
                "Up 2 minutes (unhealthy)",
                "running",
                "Em execução há 2 minutos · não saudável",
            ),
            (
                "Up 10 seconds (health: starting)",
                "running",
                "Em execução há 10 segundos · verificação de saúde em andamento",
            ),
            (
                "Up About an hour",
                "running",
                "Em execução há cerca de uma hora",
            ),
            (
                "Up About a minute",
                "running",
                "Em execução há cerca de um minuto",
            ),
            (
                "Up Less than a second",
                "running",
                "Em execução há menos de um segundo",
            ),
            ("Up 1 second", "running", "Em execução há 1 segundo"),
            ("Up 1 minute", "running", "Em execução há 1 minuto"),
            ("Up 1 hour", "running", "Em execução há 1 hora"),
            ("Up 1 day", "running", "Em execução há 1 dia"),
            ("Up 1 week", "running", "Em execução há 1 semana"),
            ("Up 1 month", "running", "Em execução há 1 mês"),
            ("Up 2 months", "running", "Em execução há 2 meses"),
            ("Up 1 year", "running", "Em execução há 1 ano"),
            ("Up 3 years", "running", "Em execução há 3 anos"),
            ("Up 1234 days", "running", "Em execução há 1.234 dias"),
            (
                "Up 3 weeks (Paused)",
                "paused",
                "Em execução há 3 semanas · pausado",
            ),
            (
                "Exited (0) 2 hours ago",
                "exited",
                "Parado há 2 horas (código 0)",
            ),
            (
                "Exited (137) About a minute ago",
                "exited",
                "Parado há cerca de um minuto (código 137)",
            ),
            (
                "Restarting (1) 5 seconds ago",
                "restarting",
                "Reiniciando há 5 segundos (código 1)",
            ),
            ("Created", "created", "Criado"),
            ("Dead", "dead", "Morto"),
            ("Removal In Progress", "removing", "Removendo"),
            ("", "exited", "parado"),
            (" ", "running", "em execução"),
            ("", "", "—"),
            // Formato desconhecido: como veio.
            ("Up 3 fortnights", "running", "Up 3 fortnights"),
            ("Up 3 days (sleepy)", "running", "Up 3 days (sleepy)"),
            ("Up", "running", "Up"),
            ("Exited (x) 2 hours ago", "exited", "Exited (x) 2 hours ago"),
            ("Exited (0) 2 hours", "exited", "Exited (0) 2 hours"),
            ("Restarting", "restarting", "Restarting"),
            ("Em pausa", "paused", "Em pausa"),
        ] {
            assert_eq!(status_label(status, state), want, "{status:?}");
        }
    }

    #[test]
    fn tasks_and_images_explain_what_they_count_on_hover() {
        let r = ResourceSnapshot {
            images_at: Some(Utc::now()),
            images: vec![DockerImage {
                id: format!("sha256:{LONG_ID}"),
                tags: vec!["web:latest".into()],
                ..Default::default()
            }],
            ..collected(eight())
        };
        let mut page = Page::new(egui::Theme::Dark, WIDE, r);
        let out = page.show();
        // Antes do ponteiro, nenhuma dica.
        assert!(!has(&out, Role::Label, TASKS_HINT));
        // (o que, papel, texto, dica)
        for (role, text, hint) in [
            (Role::Button, "Tarefas", TASKS_HINT),
            (Role::Label, "Tarefas 64", TASKS_HINT),
            (Role::Label, "Imagem / tags", IMAGES_HINT),
        ] {
            if text == "Imagem / tags" {
                page.docker.tab = Tab::Images;
            }
            let out = page.show();
            let at = rect_of(&node(&out, role, text)).center();
            let out = page.rest(at);
            node(&out, Role::Label, hint);
            page.frame(vec![Event::PointerGone]);
        }
    }

    #[test]
    fn state_labels_cover_the_docker_domain() {
        for (state, want) in [
            ("running", "em execução"),
            ("exited", "parado"),
            ("paused", "pausado"),
            ("created", "criado"),
            ("restarting", "reiniciando"),
            ("removing", "removendo"),
            ("dead", "morto"),
            ("", "—"),
            ("algo-novo", "algo-novo"),
        ] {
            assert_eq!(state_label(state), want, "{state:?}");
        }
    }

    #[test]
    fn accumulated_io_in_pt_br_with_the_direction_of_each_side() {
        for (raw, want) in [
            ("1.2GB / 840MB", "1,2 GB ↓ · 840 MB ↑"),
            ("0B / 0B", "0 B ↓ · 0 B ↑"),
            ("12.345kB / 1.5MB", "12,345 kB ↓ · 1,5 MB ↑"),
            (" 3.4GB / 9.1GB ", "3,4 GB ↓ · 9,1 GB ↑"),
            ("", "—"),
            ("  ", "—"),
            ("--", "—"),
            ("1.2GB", "1.2GB"),
            ("1.2GB / ?", "1.2GB / ?"),
            ("GB / MB", "GB / MB"),
            ("1.2 / 3", "1.2 / 3"),
        ] {
            assert_eq!(io(raw, "↓", "↑"), want, "{raw:?}");
        }
        assert_eq!(
            io("3.4GB / 9.1GB", "lidos", "gravados"),
            "3,4 GB lidos · 9,1 GB gravados"
        );
    }

    /// Para a ordenação: empate de CPU (alpha/charlie), maiúscula (Bravo),
    /// em execução sem stats (hotel) e sem tarefas (Bravo), pausado com
    /// stats (foxtrot) e parados (echo, golf).
    fn sortable() -> Vec<Container> {
        let with = |name: &str, state: &str, cpu: f64, mib: u64, tasks: Option<u64>| Container {
            usage: Some(Usage {
                cpu,
                memory: mib * MIB,
                limit: 8 * GIB,
                tasks,
            }),
            state: state.into(),
            ..running(name, 0., 0, 0, "")
        };
        vec![
            with("alpha", "running", 5., 300, Some(10)),
            with("Bravo", "running", 50., 100, None),
            with("charlie", "running", 5., 200, Some(30)),
            running("hotel", 0., 0, 0, "").tap(|c| c.usage = None),
            stopped("echo", "exited"),
            with("foxtrot", "paused", 0., 50, Some(1)),
            stopped("golf", "created"),
        ]
    }
    trait Tap: Sized {
        fn tap(mut self, f: impl FnOnce(&mut Self)) -> Self {
            f(&mut self);
            self
        }
    }
    impl Tap for Container {}

    #[test]
    fn every_sortable_column_in_both_directions_keeps_the_ones_without_a_value_last() {
        let sorted = |names: &str| names.split(' ').map(str::to_owned).collect::<Vec<_>>();
        // (clique, coluna ativa, decrescente?, ordem)
        let steps = [
            (
                None,
                "CPU",
                true,
                "Bravo alpha charlie foxtrot hotel echo golf",
            ),
            (
                Some("CPU"),
                "CPU",
                false,
                "foxtrot alpha charlie Bravo hotel echo golf",
            ),
            (
                Some("Memória / limite"),
                "Memória / limite",
                true,
                "alpha charlie Bravo foxtrot hotel echo golf",
            ),
            (
                Some("Memória / limite"),
                "Memória / limite",
                false,
                "foxtrot Bravo charlie alpha hotel echo golf",
            ),
            (
                Some("Tarefas"),
                "Tarefas",
                true,
                "charlie alpha foxtrot Bravo hotel echo golf",
            ),
            (
                Some("Tarefas"),
                "Tarefas",
                false,
                "foxtrot alpha charlie Bravo hotel echo golf",
            ),
            (
                Some("Nome"),
                "Nome",
                false,
                "alpha Bravo charlie echo foxtrot golf hotel",
            ),
            (
                Some("Nome"),
                "Nome",
                true,
                "hotel golf foxtrot echo charlie Bravo alpha",
            ),
            (
                Some("CPU"),
                "CPU",
                true,
                "Bravo alpha charlie foxtrot hotel echo golf",
            ),
        ];
        for (theme, p) in THEMES {
            let mut page = Page::new(theme, WIDE, collected(sortable()));
            for (click, active, descending, want) in steps {
                if let Some(title) = click {
                    page.click_on(Role::Button, title);
                }
                let out = page.show();
                assert_eq!(names(&out), sorted(want), "{theme:?} after {click:?}");
                // A mesma ordem com o Docker listando ao contrário.
                let forward = std::mem::replace(
                    &mut page.r,
                    collected(sortable().into_iter().rev().collect()),
                );
                assert_eq!(
                    names(&page.show()),
                    sorted(want),
                    "{theme:?} reversed listing"
                );
                page.r = forward;
                let direction = if descending {
                    SortDirection::Descending
                } else {
                    SortDirection::Ascending
                };
                for title in ["Nome", "CPU", "Memória / limite", "Tarefas"] {
                    assert_eq!(
                        node(&out, Role::Button, title).sort_direction(),
                        (title == active).then_some(direction),
                        "{theme:?} {title} after {click:?}"
                    );
                }
                let arrow = format!("{active} {}", if descending { "↓" } else { "↑" });
                assert!(
                    texts(&out).contains(&(arrow.clone(), p.text)),
                    "{theme:?}: {arrow}"
                );
                assert!(texts(&out).contains(&("Portas".to_owned(), p.text2)));
            }
        }
    }

    #[test]
    fn selection_starts_on_the_monitored_container_and_falls_back_to_the_first() {
        // O monitorado na lista: ele, mesmo não sendo o primeiro.
        let mut page = Page::new(egui::Theme::Dark, WIDE, collected(eight()));
        page.monitored = "redis-cache".into();
        let out = page.show();
        assert_eq!(names(&out)[0], "ai-memory", "precondition: not first");
        assert_eq!(selected(&out), ["redis-cache"]);
        node(
            &out,
            Role::Label,
            "Em execução há 3 dias · contêiner monitorado",
        );
        // Sem o monitorado na lista: o primeiro.
        let mut page = Page::new(egui::Theme::Dark, WIDE, collected(eight()));
        page.monitored = "nada".into();
        assert_eq!(selected(&page.show()), ["ai-memory"]);
        assert_eq!(
            page.docker.selected(),
            Some(&*format!("ai-memory-{LONG_ID}"))
        );
    }

    #[test]
    fn a_selected_container_that_leaves_the_list_goes_back_to_the_default() {
        let mut page = Page::new(egui::Theme::Light, WIDE, collected(eight()));
        page.monitored = "redis-cache".into();
        let row = row_rect(&page.show(), "postgres-dev");
        page.click(row.center());
        let out = page.show();
        assert_eq!(selected(&out), ["postgres-dev"]);
        node(&out, Role::Label, "postgres-dev");
        // Coleta nova, outra ordem e outros valores: segue o ID.
        let mut next = eight();
        next.reverse();
        next[6].usage.as_mut().unwrap().cpu = 90.;
        page.r = collected(next);
        let out = page.show();
        assert_eq!(names(&out)[0], "postgres-dev");
        assert_eq!(selected(&out), ["postgres-dev"], "follows the ID");
        // Some na coleta: volta ao padrão (o monitorado).
        page.r = collected(
            eight()
                .into_iter()
                .filter(|c| c.name != "postgres-dev")
                .collect(),
        );
        let out = page.show();
        assert_eq!(selected(&out), ["redis-cache"]);
        assert!(!has(&out, Role::Label, "postgres-dev"));
        // A busca tira o selecionado e o monitorado: o primeiro que sobra.
        page.r = collected(eight());
        page.click_row("ai-memory");
        assert_eq!(selected(&page.show()), ["ai-memory"]);
        page.docker.search = "p".into();
        let out = page.show();
        assert_eq!(names(&out), ["postgres-dev", "tempo", "zipkin"]);
        assert_eq!(selected(&out), ["postgres-dev"]);
        // A busca esvazia a lista: sem inspetor, a tabela na largura toda.
        page.docker.search = "nada assim".into();
        let out = page.show();
        assert!(rows(&out).is_empty());
        assert_eq!(page.docker.selected(), None);
        assert!(!has(&out, Role::Pane, "Inspetor do contêiner"));
        node(
            &out,
            Role::Label,
            "Nenhum contêiner corresponde à busca ou ao filtro.",
        );
        assert_eq!(
            rect_of(&node(&out, Role::Pane, "Contêineres")).width(),
            WIDE[0] - 16.
        );
        page.docker.search.clear();
        assert_eq!(selected(&page.show()), ["redis-cache"]);
        // "Só em execução" esconde o parado selecionado: volta ao padrão.
        page.click_row("minio");
        assert_eq!(selected(&page.show()), ["minio"]);
        page.click_on(Role::CheckBox, ONLY_RUNNING);
        let out = page.show();
        assert_eq!(names(&out), ["ai-memory", "postgres-dev", "redis-cache"]);
        assert_eq!(selected(&out), ["redis-cache"]);
    }

    #[test]
    fn inspector_shows_the_facts_the_monitored_processes_and_copies_exactly() {
        for (theme, p) in THEMES {
            let mut r = collected(eight());
            r.ai_processes = vec![
                ("1".into(), "ai-memory".into(), 41),
                ("27".into(), "embedder".into(), 1234),
            ];
            let mut page = Page::new(theme, WIDE, r);
            let out = page.show();
            let pane = rect_of(&node(&out, Role::Pane, "Inspetor do contêiner"));
            assert!(
                rects(&out)
                    .iter()
                    .any(|r| r.rect == pane && r.fill == p.surf),
                "{theme:?}: surf"
            );
            for text in [
                "Em execução há 3 dias · contêiner monitorado",
                "Imagem: ai-memory:latest",
                "Portas: 49374 → 49374/tcp",
                "Limite de memória: 8,0 GiB",
                "Rede acumulada: 1,2 GB ↓ · 840 MB ↑",
                "Disco acumulado: 3,4 GB lidos · 9,1 GB gravados",
                "ID: ai-memory-8f",
                "Processos no contêiner",
                "embedder",
                "27",
                "1.234",
                "Threads",
            ] {
                node(&out, Role::Label, text);
            }
            assert_eq!(
                nodes(&out, Role::Label, "ai-memory").len(),
                2,
                "name and process"
            );
            for text in ["ai-memory", "embedder"] {
                assert!(
                    pane.contains_rect(rect_of(&nodes(&out, Role::Label, text)[0])),
                    "{text} inside the inspector"
                );
            }
            let out = page.click_on(Role::Button, "Copiar ID");
            assert_eq!(copied(&out), [format!("ai-memory-{LONG_ID}")], "{theme:?}");
            let out = page.click_on(Role::Button, "Copiar nome");
            assert_eq!(copied(&out), ["ai-memory"]);
            assert!(copied(&page.show()).is_empty(), "nothing copies by itself");
        }
        // Erro do `docker top`, monitorado parado e outro contêiner.
        let mut r = collected(eight());
        r.ai_process_error = Some("Detalhamento de threads indisponível neste contêiner.".into());
        let mut page = Page::new(egui::Theme::Dark, WIDE, r);
        let out = page.show();
        node(
            &out,
            Role::Label,
            "Detalhamento de threads indisponível neste contêiner.",
        );
        assert!(!has(&out, Role::Label, "Threads"));
        // Parado, o `docker top` nem roda: sem erro e sem processos.
        page.r.ai_process_error = None;
        page.monitored = "minio".into();
        page.click_row("minio");
        let out = page.show();
        for text in [
            "Parado há 2 horas (código 0) · contêiner monitorado",
            "Limite de memória: —",
            "Rede acumulada: —",
            "Disco acumulado: —",
            "Disponível com o contêiner em execução.",
        ] {
            node(&out, Role::Label, text);
        }
        page.monitored = "outro".into();
        let out = page.show();
        node(&out, Role::Label, "Parado há 2 horas (código 0)");
        assert!(!has(&out, Role::Label, "Processos no contêiner"));
        assert!(has(&out, Role::Button, "Copiar ID"));
    }

    #[test]
    fn hidden_stopped_containers_are_counted_and_mostrar_turns_the_filter_off() {
        let mut page = Page::new(egui::Theme::Dark, WIDE, collected(eight()));
        let out = page.show();
        assert_eq!(rows(&out).len(), 8);
        assert!(!has(&out, Role::Link, "mostrar"));
        page.click_on(Role::CheckBox, ONLY_RUNNING);
        let out = page.show();
        assert!(page.docker.running_only);
        assert_eq!(names(&out), ["ai-memory", "postgres-dev", "redis-cache"]);
        node(&out, Role::Label, "5 contêineres parados ocultos");
        let line = rect_of(&node(&out, Role::Link, "mostrar"));
        assert!(line.top() >= row_rect(&out, "redis-cache").bottom());
        // Com busca: só os que ela acha contam.
        page.docker.search = "minio".into();
        let out = page.show();
        assert!(rows(&out).is_empty());
        node(&out, Role::Label, "1 contêiner parado oculto");
        node(
            &out,
            Role::Label,
            "Nenhum contêiner corresponde à busca ou ao filtro.",
        );
        page.docker.search = "ai-memory".into();
        let out = page.show();
        assert_eq!(names(&out), ["ai-memory"]);
        assert!(!has(&out, Role::Link, "mostrar"), "nothing hidden");
        page.docker.search = "i".into();
        page.click_on(Role::Link, "mostrar");
        let out = page.show();
        assert!(!page.docker.running_only);
        assert_eq!(
            names(&out),
            ["ai-memory", "redis-cache", "loki", "minio", "zipkin"]
        );
        assert_eq!(
            node(&out, Role::CheckBox, ONLY_RUNNING).toggled(),
            Some(Toggled::False)
        );
    }

    #[test]
    fn images_tab_lists_usage_and_explains_every_empty_state() {
        let now = Utc::now();
        let image = |tag: &str, size: u64, containers: &[&str], running: usize| DockerImage {
            id: format!("sha256:{tag}{}", "0".repeat(60)),
            tags: vec![format!("{tag}:latest"), format!("{tag}:v1")],
            size,
            containers: containers.iter().map(|c| c.to_string()).collect(),
            running,
        };
        for (theme, p) in THEMES {
            let r = ResourceSnapshot {
                images: vec![
                    image("web", 512 * MIB, &["web-1", "web-2"], 1),
                    image("db", GIB, &["db"], 0),
                    image("solta", 10 * MIB, &[], 0),
                ],
                images_at: Some(now),
                inventory_error: Some(
                    "Docker excedeu o limite de 6 s; nova tentativa automática.".into(),
                ),
                ..collected(vec![])
            };
            let mut page = Page::new(theme, NARROW, r);
            page.docker.tab = Tab::Images;
            let out = page.show();
            node(
                &out,
                Role::Label,
                "Inventário de imagens não atualizado. Docker excedeu o limite de 6 s; nova tentativa automática.",
            );
            assert!(
                rects(&out)
                    .iter()
                    .any(|r| r.fill == widgets::tone(p.warn, 0.10)),
                "{theme:?}: warn strip"
            );
            for text in [
                "web:latest",
                "512,0 MiB",
                "Em execução",
                "web-1, web-2",
                "db:latest",
                "1,0 GiB",
                "Associada",
                "Sem vínculo",
                "—",
                "sha256:web00000000",
            ] {
                node(&out, Role::Label, text);
            }
            let painted = texts(&out);
            assert!(
                painted.contains(&("Em execução".into(), p.acc)),
                "{theme:?}"
            );
            assert!(painted.contains(&("Associada".into(), p.text2)));
            page.docker.search = "SOLTA".into();
            let out = page.show();
            assert!(has(&out, Role::Label, "solta:latest") && !has(&out, Role::Label, "db:latest"));
            page.docker.search = "sha256:db".into();
            assert!(has(&page.show(), Role::Label, "db:latest"));
        }
        // (caso, coleta, mensagem)
        let down = ResourceSnapshot {
            at: Some(now),
            docker_error: Some("x".into()),
            ..Default::default()
        };
        let failed = ResourceSnapshot {
            inventory_error: Some("falhou".into()),
            ..collected(vec![])
        };
        let empty = ResourceSnapshot {
            images_at: Some(now),
            ..collected(vec![])
        };
        let found = ResourceSnapshot {
            images: vec![image("web", 1, &[], 0)],
            images_at: Some(now),
            ..collected(vec![])
        };
        for (case, r, search, want) in [
            (
                "docker down",
                down,
                "",
                "Docker indisponível. Veja o aviso acima.",
            ),
            (
                "first inventory failed",
                failed,
                "",
                "Inventário indisponível. Veja o aviso acima.",
            ),
            (
                "waiting",
                collected(vec![]),
                "",
                "Aguardando o inventário de imagens.",
            ),
            (
                "empty",
                empty,
                "",
                "Nenhuma imagem local neste contexto Docker.",
            ),
            (
                "no match",
                found,
                "nada",
                "Nenhuma imagem corresponde à busca.",
            ),
        ] {
            let mut page = Page::new(egui::Theme::Dark, NARROW, r);
            page.docker.tab = Tab::Images;
            page.docker.search = search.into();
            let out = page.show();
            node(&out, Role::Label, "Nenhuma imagem nesta lista");
            node(&out, Role::Label, want);
            assert!(
                has(&out, Role::Label, "Imagem / tags"),
                "{case}: the header stays"
            );
        }
    }

    #[test]
    fn processes_tab_lists_and_searches_pid_name_and_group() {
        let process = |pid: u32, name: &str, group: &str, tasks: Option<u64>| HostProcess {
            pid,
            name: name.into(),
            group: group.into(),
            usage: Usage {
                cpu: 1.25,
                memory: 64 * MIB,
                tasks,
                ..Default::default()
            },
        };
        let r = ResourceSnapshot {
            processes: vec![
                process(4321, "com.docker.backend", "Docker Desktop", Some(42)),
                process(77, "ai-memory", "ai-memory", None),
            ],
            ..collected(vec![])
        };
        let mut page = Page::new(egui::Theme::Light, NARROW, r);
        page.docker.tab = Tab::Processes;
        let out = page.show();
        for text in [
            "4321",
            "com.docker.backend",
            "1,2%",
            "64,0 MiB",
            "42",
            "77",
            "—",
        ] {
            assert!(has(&out, Role::Label, text), "{text}");
        }
        for (needle, want) in [
            ("4321", vec!["4321"]),
            ("DOCKER DESKTOP", vec!["4321"]),
            ("ai-mem", vec!["77"]),
            ("", vec!["4321", "77"]),
        ] {
            page.docker.search = needle.into();
            let out = page.show();
            let pids: Vec<&str> = ["4321", "77"]
                .into_iter()
                .filter(|pid| has(&out, Role::Label, pid))
                .collect();
            assert_eq!(pids, want, "{needle:?}");
        }
        let host = crate::platform::HOST;
        for (case, r, search, want) in [
            (
                "waiting",
                ResourceSnapshot::default(),
                "",
                "Aguardando a primeira coleta.".to_owned(),
            ),
            (
                "none",
                collected(vec![]),
                "",
                format!("Nenhum processo do Docker ou do ai-memory neste {host}."),
            ),
            (
                "no match",
                page.r.clone(),
                "nada",
                "Nenhum processo corresponde à busca.".to_owned(),
            ),
        ] {
            let mut page = Page::new(egui::Theme::Dark, NARROW, r);
            page.docker.tab = Tab::Processes;
            page.docker.search = search.into();
            let out = page.show();
            node(&out, Role::Label, "Nenhum processo nesta lista");
            assert!(has(&out, Role::Label, &want), "{case}");
        }
    }

    #[test]
    fn container_empty_states_name_the_cause() {
        let now = Utc::now();
        for (case, r, search, want) in [
            (
                "waiting",
                ResourceSnapshot::default(),
                "",
                "Aguardando a primeira coleta do Docker.",
            ),
            (
                "docker down",
                ResourceSnapshot {
                    at: Some(now),
                    docker_error: Some("x".into()),
                    ..Default::default()
                },
                "",
                "Docker indisponível. Veja o aviso acima.",
            ),
            (
                "none",
                collected(vec![]),
                "",
                "Nenhum contêiner neste contexto Docker.",
            ),
            (
                "no match",
                collected(eight()),
                "nada",
                "Nenhum contêiner corresponde à busca ou ao filtro.",
            ),
        ] {
            let mut page = Page::new(egui::Theme::Light, NARROW, r);
            page.docker.search = search.into();
            let out = page.show();
            node(&out, Role::Label, "Nenhum contêiner nesta lista");
            assert!(has(&out, Role::Label, want), "{case}");
            assert!(rows(&out).is_empty() && !has(&out, Role::Pane, "Inspetor do contêiner"));
            node(&out, Role::Button, "CPU");
        }
    }

    #[test]
    fn legend_names_the_context_and_the_inventory_age() {
        let now = Utc::now();
        for (context, images_at, want) in [
            (
                Some("desktop-linux"),
                Some(now - chrono::Duration::seconds(9)),
                [
                    "Contexto desktop-linux · CPU: 100% = 1 núcleo · memória sem cache inativo",
                    "Imagens: inventário há 9 s",
                ],
            ),
            (
                None,
                None,
                [
                    "CPU: 100% = 1 núcleo · memória sem cache inativo",
                    "Imagens: aguardando o inventário",
                ],
            ),
        ] {
            for tab in [Tab::Containers, Tab::Images, Tab::Processes] {
                let r = ResourceSnapshot {
                    docker_context: context.map(str::to_owned),
                    images_at,
                    ..collected(eight())
                };
                let mut page = Page::new(egui::Theme::Dark, NARROW, r);
                page.now = now;
                page.docker.tab = tab;
                let out = page.show();
                let lines: Vec<Rect> = want
                    .iter()
                    .map(|text| rect_of(&node(&out, Role::Label, text)))
                    .collect();
                assert!(lines[0].bottom() <= lines[1].top(), "{tab:?}: in order");
                assert!(
                    lines[1].bottom() <= NARROW[1] - 8. + 0.5,
                    "{tab:?}: inside the page"
                );
                let contexts = all(&out)
                    .iter()
                    .filter(|n| {
                        n.role() == Role::Label
                            && n.value().is_some_and(|v| v.starts_with("Contexto "))
                    })
                    .count();
                assert_eq!(contexts, usize::from(context.is_some()), "{tab:?}");
            }
        }
    }

    #[test]
    fn long_names_end_in_an_ellipsis_inside_the_row_and_show_whole_on_hover() {
        let name = "um-conteiner-com-um-nome-comprido-demais-para-a-coluna-estreita";
        let mut c = stopped(name, "exited");
        c.image = "ghcr.io/organizacao/uma-imagem-com-nome-longo:2026.11.03".into();
        let mut page = Page::new(egui::Theme::Dark, NARROW, collected(vec![c.clone()]));
        let out = page.show();
        let row = row_rect(&out, name);
        let cut: Vec<Rect> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.elided => Some(t.visual_bounding_rect()),
                _ => None,
            })
            .collect();
        assert!(cut.len() >= 2, "name and image are cut: {cut:?}");
        let name_cell = columns(row.width())[0].1;
        for rect in &cut {
            assert!(
                row.contains_rect(*rect) && rect.right() <= row.left() + name_cell - PAD + 1.,
                "{rect:?} inside the Nome cell of {row:?}"
            );
        }
        let tip = format!("{name}\n{} · parado\n—", c.image);
        assert!(!has(&out, Role::Label, &tip));
        let out = page.rest(row.center());
        node(&out, Role::Label, &tip);
    }

    #[test]
    fn only_the_visible_rows_are_mounted() {
        let many: Vec<Container> = (0..500)
            .map(|i| running(&format!("c{i:03}"), (500 - i) as f64, 1, 1, ""))
            .collect();
        let mut page = Page::new(egui::Theme::Light, WIDE, collected(many));
        let out = page.show();
        let mounted = rows(&out).len();
        assert!((5..30).contains(&mounted), "{mounted} of 500 rows mounted");
        let table = rect_of(&node(&out, Role::Pane, "Contêineres"));
        let out = page.wheel(table.center(), -1_000_000.);
        let last = row_rect(&out, "c499");
        assert!(
            table.contains_rect(last),
            "the bottom is reachable: {last:?}"
        );
        assert!(rows(&out).len() < 30, "still only what is visible");
        assert!(!names(&out).contains(&"c000".to_owned()));
    }

    #[test]
    fn search_is_typed_in_the_field_matches_name_image_and_id_and_is_kept_per_tab() {
        let mut page = Page::new(egui::Theme::Light, WIDE, collected(eight()));
        for (needle, want) in [
            ("REDIS", vec!["redis-cache"]),
            ("postgres-dev:latest", vec!["postgres-dev"]),
            ("minio-8f3c", vec!["minio"]),
            (
                "",
                vec![
                    "ai-memory",
                    "postgres-dev",
                    "redis-cache",
                    "grafana",
                    "loki",
                    "minio",
                    "tempo",
                    "zipkin",
                ],
            ),
        ] {
            page.docker.search = needle.into();
            assert_eq!(names(&page.show()), want, "{needle:?}");
        }
        let field = |out: &egui::FullOutput| {
            all(out)
                .into_iter()
                .find(|n| n.role() == Role::TextInput && n.placeholder() == Some(SEARCH))
                .map(|n| rect_of(&n))
                .expect("the search field")
        };
        let at = field(&page.show()).center();
        page.click(at);
        page.frame(vec![Event::Text("redis".into())]);
        assert_eq!(page.docker.search, "redis");
        assert_eq!(names(&page.show()), ["redis-cache"]);
        // A mesma busca vale nas outras abas e fica ao voltar.
        page.click_on(Role::Button, "Imagens —");
        let out = page.show();
        assert_eq!(page.docker.tab, Tab::Images);
        node(&out, Role::Label, "Nenhuma imagem nesta lista");
        assert_eq!(page.docker.search, "redis");
    }

    #[test]
    fn reset_clears_only_the_selection_and_open_shows_a_container() {
        let mut docker = Docker {
            tab: Tab::Images,
            search: "busca".into(),
            running_only: true,
            sort: Sort {
                col: Col::Name,
                descending: false,
            },
            selected: Some("id".into()),
        };
        docker.reset();
        assert_eq!(
            (
                docker.tab,
                docker.search.as_str(),
                docker.running_only,
                docker.sort,
                docker.selected.clone()
            ),
            (
                Tab::Images,
                "busca",
                true,
                Sort {
                    col: Col::Name,
                    descending: false
                },
                None
            )
        );
        docker.open("outro");
        assert_eq!(
            (
                docker.tab,
                docker.search.as_str(),
                docker.selected.as_deref()
            ),
            (Tab::Containers, "", Some("outro"))
        );
        assert_eq!(
            Docker::default().sort,
            Sort {
                col: Col::Cpu,
                descending: true
            }
        );
        assert!(!Docker::default().running_only);
    }

    #[test]
    fn inspector_is_beside_when_wide_and_below_the_rows_when_narrow() {
        for (theme, _) in THEMES {
            let mut page = Page::new(theme, WIDE, collected(eight()));
            let out = page.show();
            let table = rect_of(&node(&out, Role::Pane, "Contêineres"));
            let inspector = rect_of(&node(&out, Role::Pane, "Inspetor do contêiner"));
            assert_eq!(inspector.width(), INSPECTOR, "{theme:?}");
            assert_eq!(inspector.right(), WIDE[0] - 8.);
            assert_eq!(table.left(), 8.);
            assert_eq!(table.right() + 1., inspector.left());
            assert_eq!(inspector.top(), table.top());
            // Estreita: a tabela na largura toda; o inspetor depois da última
            // linha, na mesma rolagem.
            page.size = NARROW;
            let out = page.show();
            let table = rect_of(&node(&out, Role::Pane, "Contêineres"));
            assert_eq!((table.left(), table.right()), (8., NARROW[0] - 8.));
            let out = page.wheel(table.center(), -2000.);
            let inspector = rect_of(&node(&out, Role::Pane, "Inspetor do contêiner"));
            let last = row_rect(&out, "zipkin");
            assert!(
                inspector.top() >= last.bottom(),
                "{inspector:?} below {last:?}"
            );
            assert!(
                (inspector.left() - table.left()).abs() < 0.5
                    && inspector.right() <= table.right() + 0.5
                    && inspector.width() > table.width() - 20.,
                "{theme:?}: {inspector:?} spans {table:?}"
            );
            let copy = rect_of(&node(&out, Role::Button, "Copiar nome"));
            assert!(
                table.contains_rect(copy),
                "reachable: {copy:?} in {table:?}"
            );
            let out = page.click(copy.center());
            assert_eq!(copied(&out), ["ai-memory"]);
        }
    }
}
