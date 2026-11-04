//! O menu da barra de menus (macOS) e da bandeja (Linux): uma descrição só,
//! que as duas plataformas só desenham. `MenuReading` é a leitura (recursos,
//! saúde, handoffs e propostas, sob a config); `entries` diz as linhas, na
//! ordem; `Entry::flat` é a forma em texto (bandeja e `--print-menu`).
use crate::model::{ApiSnapshot, Config, ResourceSnapshot, Usage, bytes, decimal, number};
use chrono::{DateTime, Utc};

/// O traço do ícone (o pulso do artboard, `M2 13h4l2.5-6 4 11 3-7H22`), em 24
/// unidades: o item da barra, a bandeja e a prévia em Ajustes.
pub const PULSE: [(f32, f32); 6] = [
    (2., 13.),
    (6., 13.),
    (8.5, 7.),
    (12.5, 18.),
    (15.5, 11.),
    (22., 11.),
];
/// A espessura do traço do pulso, nas mesmas 24 unidades.
pub const PULSE_WIDTH: f32 = 2.4;
/// Métrica sem medida (nunca zero), na linha em texto e na do macOS.
pub const UNAVAILABLE: &str = "indisponível";
/// O âmbar do ponto de alerta no ícone (MenuBar.dc.html), nos dois sistemas.
pub const ALERT_DOT: (u8, u8, u8) = (0xF2, 0xA3, 0x3A);

/// Para onde a barra e a bandeja levam o painel. O código vai por
/// `Shared::navigation` (macOS) e por `commands::Command::Open` (Linux), e o
/// painel o lê em `MonitorApp::navigate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Só mostra o painel, na página em que estiver.
    Panel,
    Health,
    Review,
    Sessions,
    /// O painel e, por cima, a janela de Ajustes.
    Settings,
}
impl Destination {
    pub fn code(self) -> u8 {
        match self {
            Destination::Panel => 0,
            Destination::Health => 1,
            Destination::Review => 2,
            Destination::Sessions => 3,
            Destination::Settings => 4,
        }
    }
    /// O destino de um código; fora do domínio, nenhum.
    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Destination::Panel,
            1 => Destination::Health,
            2 => Destination::Review,
            3 => Destination::Sessions,
            4 => Destination::Settings,
            _ => return None,
        })
    }
}

/// O que uma linha do menu faz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Open(Destination),
    Quit,
}
impl MenuAction {
    /// Nome estável da ação no `--print-menu` (o smoke da bandeja clica por ele).
    fn id(self) -> &'static str {
        match self {
            MenuAction::Open(Destination::Panel) => "panel",
            MenuAction::Open(Destination::Health) => "health",
            MenuAction::Open(Destination::Review) => "review",
            MenuAction::Open(Destination::Sessions) => "sessions",
            MenuAction::Open(Destination::Settings) => "settings",
            MenuAction::Quit => "quit",
        }
    }
}

/// O estado da coleta de recursos, no cabeçalho; a idade em segundos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Waiting,
    Live(i64),
    Late(i64),
    DockerDown(i64),
}
impl State {
    pub fn text(self) -> String {
        use crate::app::ago;
        match self {
            State::Waiting => "aguardando a primeira coleta".into(),
            State::Live(age) => format!("ao vivo · {}", ago(age)),
            State::Late(age) => format!("atrasado · {}", ago(age)),
            State::DockerDown(age) => format!("Docker indisponível · {}", ago(age)),
        }
    }
}

/// CPU (100% = 1 núcleo) e memória medidas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Load {
    pub cpu: f64,
    pub memory: u64,
}

/// Propostas pendentes do projeto vigiado (Ajustes › Barra de menus).
#[derive(Clone, Debug, PartialEq)]
pub enum Proposals {
    /// Sem leitura da saúde.
    Waiting,
    NoProject,
    /// `pending` = `None`: desconhecido (atrasada, recusada, de outro projeto).
    Watched {
        scope: String,
        pending: Option<usize>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuReading {
    pub state: State,
    /// Só de uma amostra em dia; `None` = indisponível, nunca zero.
    pub ai: Option<Load>,
    pub docker: Option<Load>,
    /// Contêineres em execução; `None` = Docker indisponível ou amostra velha.
    pub running: Option<usize>,
    /// Alertas da saúde, só de uma leitura em dia.
    pub alerts: Vec<String>,
    /// O resumo da saúde, para a dica.
    pub health: String,
    /// Handoffs abertos, só de uma coleta da API em dia.
    pub handoffs: Option<usize>,
    pub proposals: Proposals,
    /// O ponto âmbar no ícone: alertas e a opção ligada em Ajustes.
    pub alert_dot: bool,
}

impl MenuReading {
    pub fn from_snapshot(
        snapshot: &ResourceSnapshot,
        container: &str,
        interval: u64,
        now: DateTime<Utc>,
    ) -> Self {
        let age = snapshot.at.map(|at| (now - at).num_seconds().max(0));
        let fresh = age.is_some_and(|age| !crate::app::is_late(age, interval));
        let load = |u: Usage| Load {
            cpu: u.cpu,
            memory: u.memory,
        };
        Self {
            state: match age {
                None => State::Waiting,
                Some(age) if !fresh => State::Late(age),
                Some(age) if snapshot.docker_error.is_some() => State::DockerDown(age),
                Some(age) => State::Live(age),
            },
            ai: fresh
                .then(|| snapshot.ai_usage(container))
                .flatten()
                .map(load),
            docker: fresh.then(|| snapshot.docker_total()).flatten().map(load),
            running: (fresh && snapshot.docker_error.is_none()).then(|| {
                snapshot
                    .containers
                    .iter()
                    .filter(|c| c.state == "running")
                    .count()
            }),
            alerts: Vec::new(),
            health: "Saúde: aguardando".into(),
            handoffs: None,
            proposals: Proposals::Waiting,
            alert_dot: false,
        }
    }
    pub fn with_context(
        mut self,
        api: Option<&ApiSnapshot>,
        health: Option<&crate::health::Snapshot>,
        config: &Config,
        now: DateTime<Utc>,
    ) -> Self {
        if let Some(health) = health {
            if health.fresh(now) {
                self.alerts = health.alerts();
            }
            self.health = health.menu_summary(now);
        }
        self.alert_dot = !self.alerts.is_empty() && config.alert_dot;
        self.handoffs = api.and_then(|a| {
            a.at.filter(|at| !crate::app::is_late((now - *at).num_seconds(), config.api_seconds))
                .and(a.handoff_total)
        });
        self.proposals = match (health, &config.proposal_watch) {
            (None, _) => Proposals::Waiting,
            (Some(_), None) => Proposals::NoProject,
            (Some(health), Some(scope)) => Proposals::Watched {
                scope: scope.label(),
                pending: health.watched_pending(scope, now),
            },
        };
        self
    }
}

/// A leitura de um tique da barra ou da bandeja, com o modo do item. `None`
/// quando algum lock está ocupado (um coletor gravando, a config mudando):
/// nunca espera e quem chama mantém a leitura anterior, em vez de mostrar
/// por um tique "sem saúde" ou "sem API".
pub fn read(
    shared: &crate::monitor::Shared,
    now: DateTime<Utc>,
) -> Option<(MenuReading, crate::model::BarMode)> {
    let config = shared.config.try_read().ok()?;
    let snapshot = shared.resources.try_lock().ok()?;
    let api = shared.api.try_lock().ok()?;
    let health = shared.health.try_lock().ok()?;
    let reading =
        MenuReading::from_snapshot(&snapshot, &config.container, config.resource_seconds, now)
            .with_context(Some(&api), Some(&health), &config, now);
    Some((reading, config.bar_mode))
}

/// Uma linha do menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// "AI Monitor" e o estado da coleta.
    Header(State),
    /// O nome e, medidos, CPU e memória em texto; `None` = indisponível.
    Metric {
        name: String,
        value: Option<(String, String)>,
    },
    /// "Saúde: …" com os alertas; só visível quando há algum.
    Alert(String),
    Separator,
    /// Uma ação; `detail` vai à direita (o projeto das propostas).
    Action {
        label: String,
        detail: Option<String>,
        action: MenuAction,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub row: Row,
    pub visible: bool,
    /// O texto (ou a visibilidade) depende da leitura ou da config.
    pub dynamic: bool,
}

/// O menu, linha a linha, na ordem: cabeçalho, métricas, a faixa de alerta,
/// handoffs e propostas, as páginas, Ajustes e Sair. O número e a ordem das
/// linhas não mudam com a leitura; a faixa só fica invisível.
pub fn entries(r: &MenuReading) -> Vec<Entry> {
    let entry = |row, dynamic| Entry {
        row,
        visible: true,
        dynamic,
    };
    let action = |label: &str, action| {
        entry(
            Row::Action {
                label: label.into(),
                detail: None,
                action,
            },
            false,
        )
    };
    let (proposals, scope) = proposals(&r.proposals);
    vec![
        entry(Row::Header(r.state), true),
        entry(
            Row::Metric {
                name: "ai-memory".into(),
                value: r.ai.map(cells),
            },
            true,
        ),
        entry(
            Row::Metric {
                name: docker_name(r.running),
                value: r.docker.map(cells),
            },
            true,
        ),
        Entry {
            row: Row::Alert(alert(&r.alerts)),
            visible: !r.alerts.is_empty(),
            dynamic: true,
        },
        entry(Row::Separator, false),
        entry(
            Row::Action {
                label: handoffs(r.handoffs),
                detail: None,
                action: MenuAction::Open(Destination::Sessions),
            },
            true,
        ),
        entry(
            Row::Action {
                label: proposals,
                detail: scope,
                action: MenuAction::Open(Destination::Review),
            },
            true,
        ),
        entry(Row::Separator, false),
        action("Abrir painel", MenuAction::Open(Destination::Panel)),
        action("Saúde do ai-memory", MenuAction::Open(Destination::Health)),
        action("Revisar propostas…", MenuAction::Open(Destination::Review)),
        entry(Row::Separator, false),
        action("Ajustes…", MenuAction::Open(Destination::Settings)),
        action("Sair do AI Monitor", MenuAction::Quit),
    ]
}

/// Uma linha em texto: o rótulo da bandeja e do `--print-menu`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Flat {
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub dynamic: bool,
    pub visible: bool,
    /// A ação (`MenuAction::id`); `None` nas linhas informativas.
    pub action: Option<&'static str>,
}
impl Entry {
    pub fn flat(&self) -> Flat {
        let (label, action) = match &self.row {
            Row::Header(state) => (format!("AI Monitor · {}", state.text()), None),
            Row::Metric {
                name,
                value: Some((cpu, memory)),
            } => (format!("{name} · {cpu} · {memory}"), None),
            Row::Metric { name, value: None } => (format!("{name} · {UNAVAILABLE}"), None),
            Row::Alert(text) => (text.clone(), None),
            Row::Separator => (String::new(), None),
            Row::Action {
                label,
                detail,
                action,
            } => {
                let (main, rest) = action_text(label, detail.as_deref());
                (main + &rest.unwrap_or_default(), Some(*action))
            }
        };
        Flat {
            label,
            enabled: action.is_some(),
            separator: self.row == Row::Separator,
            dynamic: self.dynamic,
            visible: self.visible,
            action: action.map(MenuAction::id),
        }
    }
}

/// O texto de uma ação e, com `detail`, o resto (" · acme / api"), que o
/// macOS pinta em cor secundária logo depois do rótulo; juntos, são a linha
/// em texto da bandeja.
pub fn action_text(label: &str, detail: Option<&str>) -> (String, Option<String>) {
    (label.to_owned(), detail.map(|d| format!(" · {d}")))
}

/// A dica do ícone: as linhas da leitura, o resumo da saúde e as legendas,
/// que ficam fora do menu.
pub fn tooltip(r: &MenuReading) -> String {
    let mut lines: Vec<String> = entries(r)
        .iter()
        .filter(|e| e.dynamic && !matches!(e.row, Row::Alert(_)))
        .map(|e| e.flat().label)
        .collect();
    lines.push(r.health.clone());
    lines.push("CPU: 100% = 1 núcleo · M/G = MiB/GiB".into());
    lines.push("Docker = contêineres ativos, incluindo ai-memory".into());
    lines.join("\n")
}

/// O `--print-menu`: as linhas em texto, em JSON, de uma leitura sem dados
/// sob `config` (o que a bandeja mostra ao iniciar).
pub fn print_menu(config: &Config) -> String {
    let now = Utc::now();
    let reading = MenuReading::from_snapshot(
        &ResourceSnapshot::default(),
        &config.container,
        config.resource_seconds,
        now,
    )
    .with_context(
        Some(&ApiSnapshot::default()),
        Some(&crate::health::Snapshot::default()),
        config,
        now,
    );
    let flat: Vec<Flat> = entries(&reading).iter().map(Entry::flat).collect();
    serde_json::to_string_pretty(&flat).expect("menu lines are plain JSON")
}

fn cells(load: Load) -> (String, String) {
    (format!("{}%", decimal(load.cpu, 1)), memory(load.memory))
}
/// A memória no menu (MenuBar.dc.html): MiB inteiros abaixo de 1 GiB, GiB com
/// uma casa a partir dele; abaixo de 1 MiB, como `model::bytes`.
fn memory(n: u64) -> String {
    if ((1 << 20)..(1 << 30)).contains(&n) {
        format!("{} MiB", decimal(n as f64 / (1u64 << 20) as f64, 0))
    } else {
        bytes(n)
    }
}
fn docker_name(running: Option<usize>) -> String {
    match running {
        Some(1) => "Docker · 1 ativo".into(),
        Some(n) => format!("Docker · {} ativos", number(n)),
        None => "Docker".into(),
    }
}
/// "Saúde: " e os alertas; sem o prefixo quando eles já começam por "Saúde"
/// ("Saúde do servidor indisponível").
fn alert(alerts: &[String]) -> String {
    let text = alerts.join(" · ");
    if text.is_empty() || text.starts_with("Saúde") {
        text
    } else {
        format!("Saúde: {text}")
    }
}
fn handoffs(open: Option<usize>) -> String {
    match open {
        Some(1) => "1 handoff aberto".into(),
        Some(n) => format!("{} handoffs abertos", number(n)),
        None => "Handoffs abertos: —".into(),
    }
}
/// O texto das propostas e o projeto, que vai à direita.
fn proposals(p: &Proposals) -> (String, Option<String>) {
    match p {
        Proposals::Waiting => ("Propostas: aguardando".into(), None),
        Proposals::NoProject => ("Propostas: selecione um projeto em Ajustes".into(), None),
        Proposals::Watched { scope, pending } => (
            match pending {
                Some(1) => "1 proposta pendente".into(),
                Some(n) => format!("{} propostas pendentes", crate::health::pending_text(*n)),
                None => "Propostas: —".into(),
            },
            Some(scope.clone()),
        ),
    }
}

/// O texto do item da barra de menus e se ele fica esmaecido (algum número
/// que o modo mostra está indisponível ou atrasado).
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, PartialEq)]
pub struct BarTitle {
    pub text: String,
    pub dimmed: bool,
}
/// O item da barra no modo `mode`: o mesmo texto na barra e na prévia de
/// Ajustes. Só o ícone e Ícone + CPU esmaecem sem a CPU do ai-memory.
#[cfg(any(target_os = "macos", test))]
pub fn bar_title(r: &MenuReading, mode: crate::model::BarMode) -> BarTitle {
    use crate::model::BarMode;
    match mode {
        BarMode::Icon => BarTitle {
            text: String::new(),
            dimmed: r.ai.is_none(),
        },
        BarMode::IconCpu => BarTitle {
            text: r
                .ai
                .map(|ai| format!("{}%", decimal(ai.cpu, 0)))
                .unwrap_or("—".into()),
            dimmed: r.ai.is_none(),
        },
        BarMode::Full => BarTitle {
            text: format!("AI {} | Docker {}", compact(r.ai), compact(r.docker)),
            dimmed: r.ai.is_none() || r.docker.is_none(),
        },
    }
}
#[cfg(any(target_os = "macos", test))]
fn compact(load: Option<Load>) -> String {
    load.map(|l| format!("{}% {}", decimal(l.cpu, 0), compact_bytes(l.memory)))
        .unwrap_or("—".into())
}
#[cfg(any(target_os = "macos", test))]
fn compact_bytes(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{}G", decimal(n as f64 / (1u64 << 30) as f64, 1))
    } else {
        format!("{}M", decimal(n as f64 / (1u64 << 20) as f64, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BarMode, Container};
    use chrono::Duration;

    const MIB: u64 = 1 << 20;

    fn container(name: &str, state: &str, cpu: f64, memory: u64) -> Container {
        Container {
            name: name.into(),
            state: state.into(),
            usage: Some(Usage {
                cpu,
                memory,
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    /// Três em execução (a soma: 15,5% e 733 MiB) e um parado, de 1 s atrás.
    fn resources(now: DateTime<Utc>) -> ResourceSnapshot {
        ResourceSnapshot {
            at: Some(now - Duration::seconds(1)),
            containers: vec![
                container("ai-memory", "running", 12.4, 421 * MIB),
                container("db", "running", 3.1, 312 * MIB),
                container("cache", "running", 0., 0),
                container("old", "exited", 50., 900 * MIB),
            ],
            ..Default::default()
        }
    }
    fn scope() -> crate::client::Scope {
        crate::client::Scope {
            workspace: "acme".into(),
            project: "api-pagamentos".into(),
        }
    }
    fn watching() -> Config {
        Config {
            proposal_watch: Some(scope()),
            ..Default::default()
        }
    }
    fn health(now: DateTime<Utc>, status: serde_json::Value) -> crate::health::Snapshot {
        crate::health::Snapshot {
            at: Some(now),
            status: Some(status),
            pending: Some(3),
            pending_scope: Some(scope()),
            ..Default::default()
        }
    }
    fn api(now: DateTime<Utc>, open: usize) -> ApiSnapshot {
        ApiSnapshot {
            at: Some(now),
            handoff_total: Some(open),
            ..Default::default()
        }
    }
    fn full(now: DateTime<Utc>) -> MenuReading {
        MenuReading::from_snapshot(&resources(now), "ai-memory", 2, now).with_context(
            Some(&api(now, 75)),
            Some(&health(
                now,
                serde_json::json!({"consolidation": {"failed": 2}}),
            )),
            &watching(),
            now,
        )
    }
    fn flats(r: &MenuReading) -> Vec<Flat> {
        entries(r).iter().map(Entry::flat).collect()
    }
    fn label_of(r: &MenuReading, action: &str) -> String {
        flats(r)
            .into_iter()
            .find(|f| f.action == Some(action) && f.dynamic)
            .expect("a dynamic action row")
            .label
    }

    /// A única lista literal do menu: a bandeja, o macOS e o `--print-menu`
    /// saem de `entries`, e o smoke da bandeja compara com o `--print-menu`.
    #[test]
    fn the_menu_lines_in_order_with_their_flags_and_actions() {
        let now = Utc::now();
        let got: Vec<_> = flats(&full(now))
            .into_iter()
            .map(|f| {
                (
                    f.label,
                    f.enabled,
                    f.separator,
                    f.dynamic,
                    f.visible,
                    f.action,
                )
            })
            .collect();
        let line = |label: &str, enabled, dynamic, action| {
            (label.to_owned(), enabled, false, dynamic, true, action)
        };
        let separator = (String::new(), false, true, false, true, None);
        assert_eq!(
            got,
            [
                line("AI Monitor · ao vivo · há 1 s", false, true, None),
                line("ai-memory · 12,4% · 421 MiB", false, true, None),
                line("Docker · 3 ativos · 15,5% · 733 MiB", false, true, None),
                line("Saúde: Consolidações com falha", false, true, None),
                separator.clone(),
                line("75 handoffs abertos", true, true, Some("sessions")),
                line(
                    "3 propostas pendentes · acme / api-pagamentos",
                    true,
                    true,
                    Some("review")
                ),
                separator.clone(),
                line("Abrir painel", true, false, Some("panel")),
                line("Saúde do ai-memory", true, false, Some("health")),
                line("Revisar propostas…", true, false, Some("review")),
                separator,
                line("Ajustes…", true, false, Some("settings")),
                line("Sair do AI Monitor", true, false, Some("quit")),
            ]
        );
    }

    #[test]
    fn the_project_follows_the_label_as_a_secondary_tail() {
        let entries = entries(&full(Utc::now()));
        let parts: Vec<(String, Option<String>, String)> = entries
            .iter()
            .filter_map(|e| match &e.row {
                Row::Action { label, detail, .. } if e.dynamic => {
                    let (main, rest) = action_text(label, detail.as_deref());
                    Some((main, rest, e.flat().label))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            parts,
            [
                (
                    "75 handoffs abertos".into(),
                    None,
                    "75 handoffs abertos".into()
                ),
                (
                    "3 propostas pendentes".into(),
                    Some(" · acme / api-pagamentos".into()),
                    "3 propostas pendentes · acme / api-pagamentos".into()
                ),
            ]
        );
    }

    #[test]
    fn every_action_is_its_destination_and_every_code_is_one_destination() {
        // A ação de cada linha habilitada, na ordem do menu.
        let actions: Vec<MenuAction> = entries(&full(Utc::now()))
            .into_iter()
            .filter_map(|e| match e.row {
                Row::Action { action, .. } => Some(action),
                _ => None,
            })
            .collect();
        use Destination::*;
        assert_eq!(
            actions,
            [
                MenuAction::Open(Sessions),
                MenuAction::Open(Review),
                MenuAction::Open(Panel),
                MenuAction::Open(Health),
                MenuAction::Open(Review),
                MenuAction::Open(Settings),
                MenuAction::Quit,
            ]
        );
        let codes = [
            (Panel, 0),
            (Health, 1),
            (Review, 2),
            (Sessions, 3),
            (Settings, 4),
        ];
        for (destination, code) in codes {
            // Um destino novo tem de aparecer nesta matriz.
            match destination {
                Panel | Health | Review | Sessions | Settings => {}
            }
            assert_eq!(destination.code(), code, "{destination:?}");
        }
        for code in 0..=u8::MAX {
            let want = codes.iter().find(|(_, c)| *c == code).map(|(d, _)| *d);
            assert_eq!(Destination::from_code(code), want, "code {code}");
        }
    }

    #[test]
    fn the_header_names_each_collection_state() {
        let now = Utc::now();
        let at = |secs: i64| Some(now - Duration::seconds(secs));
        let down = |at| ResourceSnapshot {
            at,
            docker_error: Some("offline".into()),
            ..Default::default()
        };
        // (caso, amostra, intervalo, cabeçalho)
        for (case, snapshot, interval, want) in [
            (
                "no sample yet",
                ResourceSnapshot::default(),
                2,
                "AI Monitor · aguardando a primeira coleta",
            ),
            ("live", resources(now), 2, "AI Monitor · ao vivo · há 1 s"),
            (
                "10 s is the minimum window",
                ResourceSnapshot {
                    at: at(10),
                    ..Default::default()
                },
                2,
                "AI Monitor · ao vivo · há 10 s",
            ),
            (
                "11 s is late at 2 s",
                ResourceSnapshot {
                    at: at(11),
                    ..Default::default()
                },
                2,
                "AI Monitor · atrasado · há 11 s",
            ),
            (
                "three intervals of 10 s",
                ResourceSnapshot {
                    at: at(30),
                    ..Default::default()
                },
                10,
                "AI Monitor · ao vivo · há 30 s",
            ),
            (
                "late in minutes",
                ResourceSnapshot {
                    at: at(125),
                    ..Default::default()
                },
                10,
                "AI Monitor · atrasado · há 2 min",
            ),
            (
                "Docker down",
                down(at(1)),
                2,
                "AI Monitor · Docker indisponível · há 1 s",
            ),
            (
                "late wins over Docker down",
                down(at(60)),
                2,
                "AI Monitor · atrasado · há 1 min",
            ),
        ] {
            let reading = MenuReading::from_snapshot(&snapshot, "ai-memory", interval, now);
            assert_eq!(flats(&reading)[0].label, want, "{case}");
        }
    }

    #[test]
    fn samples_are_late_after_three_intervals_and_never_before_10_s() {
        let now = Utc::now();
        // (intervalo, idade, atrasada?): com 2 s, 3 × 2 = 6 fica abaixo do
        // piso de 10 s; com 60 s, valem os 180.
        for (interval, age, late) in [
            (2, 6, false),
            (2, 7, false),
            (2, 10, false),
            (2, 11, true),
            (60, 180, false),
            (60, 181, true),
        ] {
            let mut snapshot = resources(now);
            snapshot.at = Some(now - Duration::seconds(age));
            let reading = MenuReading::from_snapshot(&snapshot, "ai-memory", interval, now);
            let case = format!("{interval} s, {age} s old");
            if late {
                assert_eq!(reading.state, State::Late(age), "{case}");
                assert_eq!((reading.ai, reading.docker), (None, None), "{case}");
            } else {
                assert_eq!(reading.state, State::Live(age), "{case}");
                let ai = Load {
                    cpu: 12.4,
                    memory: 421 * MIB,
                };
                assert_eq!(reading.ai, Some(ai), "{case}");
            }
        }
        // A API pela mesma regra: no menor intervalo (5 s), 3 × 5 = 15 já
        // passa do piso; em 60 s, os 180.
        for (api_seconds, age, late) in [
            (5, 10, false),
            (5, 15, false),
            (5, 16, true),
            (60, 180, false),
            (60, 181, true),
        ] {
            let config = Config {
                api_seconds,
                ..Default::default()
            };
            let api = ApiSnapshot {
                at: Some(now - Duration::seconds(age)),
                handoff_total: Some(5),
                ..Default::default()
            };
            let reading = MenuReading::from_snapshot(&resources(now), "ai-memory", 2, now)
                .with_context(Some(&api), None, &config, now);
            let want = (!late).then_some(5);
            assert_eq!(reading.handoffs, want, "API {api_seconds} s, {age} s old");
        }
    }

    #[test]
    fn unavailable_metrics_never_read_as_zero() {
        let now = Utc::now();
        let metrics = |r: &MenuReading| -> Vec<String> {
            flats(r)[1..3].iter().map(|f| f.label.clone()).collect()
        };
        // Docker sem contêineres em execução: zero medido.
        let empty = ResourceSnapshot {
            at: Some(now),
            ..Default::default()
        };
        assert_eq!(
            metrics(&MenuReading::from_snapshot(&empty, "ai-memory", 2, now)),
            ["ai-memory · indisponível", "Docker · 0 ativos · 0,0% · 0 B"]
        );
        // Um em execução, com zero de CPU e memória.
        let idle = ResourceSnapshot {
            at: Some(now),
            containers: vec![container("ai-memory", "running", 0., 0)],
            ..Default::default()
        };
        assert_eq!(
            metrics(&MenuReading::from_snapshot(&idle, "ai-memory", 2, now)),
            ["ai-memory · 0,0% · 0 B", "Docker · 1 ativo · 0,0% · 0 B"]
        );
        // Docker fora: sem contagem e sem números.
        let mut down = resources(now);
        down.docker_error = Some("offline".into());
        assert_eq!(
            metrics(&MenuReading::from_snapshot(&down, "ai-memory", 2, now)),
            ["ai-memory · indisponível", "Docker · indisponível"]
        );
        // Amostra velha: os números somem, mesmo com tudo medido nela.
        let mut old = resources(now);
        old.at = Some(now - Duration::seconds(30));
        assert_eq!(
            metrics(&MenuReading::from_snapshot(&old, "ai-memory", 2, now)),
            ["ai-memory · indisponível", "Docker · indisponível"]
        );
        // Um contêiner em execução sem estatística: contado, sem soma.
        let mut partial = resources(now);
        partial.containers[1].usage = None;
        assert_eq!(
            metrics(&MenuReading::from_snapshot(&partial, "ai-memory", 2, now)),
            [
                "ai-memory · 12,4% · 421 MiB",
                "Docker · 3 ativos · indisponível"
            ]
        );
        // Milhar e GiB.
        let big = ResourceSnapshot {
            at: Some(now),
            containers: (0..1234)
                .map(|i| container(&format!("c{i}"), "running", 1., 2 * MIB))
                .collect(),
            ..Default::default()
        };
        assert_eq!(
            metrics(&MenuReading::from_snapshot(&big, "c0", 2, now)),
            [
                "ai-memory · 1,0% · 2 MiB",
                "Docker · 1.234 ativos · 1.234,0% · 2,4 GiB"
            ]
        );
    }

    #[test]
    fn memory_is_whole_mib_below_one_gib_and_one_decimal_from_it() {
        const GIB: u64 = 1 << 30;
        for (n, want) in [
            (0, "0 B"),
            (1023, "1.023 B"),
            (MIB - 1, "1.024,0 KiB"),
            (MIB, "1 MiB"),
            (MIB + MIB / 2 - 1, "1 MiB"),
            (421 * MIB, "421 MiB"),
            (1000 * MIB, "1.000 MiB"),
            // Arredonda como o resto: logo abaixo de 1 GiB ainda é MiB.
            (GIB - 1, "1.024 MiB"),
            (GIB, "1,0 GiB"),
            (GIB + GIB * 6 / 10, "1,6 GiB"),
            (1500 * GIB, "1.500,0 GiB"),
        ] {
            assert_eq!(memory(n), want, "{n} bytes");
        }
        // O `bytes` do resto do app não muda.
        assert_eq!(bytes(421 * MIB), "421,0 MiB");
        assert_eq!(bytes(MIB), "1,0 MiB");
    }

    #[test]
    fn the_band_says_health_once() {
        for (alerts, want) in [
            (vec![], ""),
            (
                vec!["Consolidações com falha"],
                "Saúde: Consolidações com falha",
            ),
            (
                vec!["Saúde do servidor indisponível"],
                "Saúde do servidor indisponível",
            ),
            (
                vec!["Consolidações com falha", "Saúde do servidor indisponível"],
                "Saúde: Consolidações com falha · Saúde do servidor indisponível",
            ),
        ] {
            let alerts: Vec<String> = alerts.into_iter().map(String::from).collect();
            assert_eq!(alert(&alerts), want, "{alerts:?}");
        }
        // Do começo: o servidor fora vira a faixa sem "Saúde: Saúde".
        let now = Utc::now();
        let down = crate::health::Snapshot {
            at: Some(now),
            error: Some(crate::client::Error::Unavailable),
            ..Default::default()
        };
        let reading = MenuReading::from_snapshot(&resources(now), "ai-memory", 2, now)
            .with_context(None, Some(&down), &Config::default(), now);
        assert_eq!(
            reading.alerts,
            ["Saúde do servidor indisponível"],
            "health.alerts() keeps its text"
        );
        let band = &flats(&reading)[3];
        assert_eq!(
            (band.label.as_str(), band.visible),
            ("Saúde do servidor indisponível", true)
        );
    }

    #[test]
    fn a_busy_lock_skips_the_tick_instead_of_reading_as_no_data() {
        use crate::monitor::Shared;
        let now = Utc::now();
        let shared = Shared::unstarted(Config {
            bar_mode: crate::model::BarMode::Full,
            ..watching()
        });
        *shared.resources.lock().unwrap() = resources(now);
        *shared.api.lock().unwrap() = api(now, 75);
        *shared.health.lock().unwrap() =
            health(now, serde_json::json!({"consolidation": {"failed": 2}}));
        let (reading, mode) = read(&shared, now).expect("all locks free");
        assert_eq!(reading, full(now), "the same reading as with_context");
        assert_eq!(mode, crate::model::BarMode::Full);
        assert!(reading.alert_dot, "precondition: the alert");
        // Cada lock ocupado: nenhum tique, nunca uma leitura sem alertas.
        let busy = |what: &str| {
            let skipped = match what {
                "config" => {
                    let _held = shared.config.write().unwrap();
                    read(&shared, now)
                }
                "resources" => {
                    let _held = shared.resources.lock().unwrap();
                    read(&shared, now)
                }
                "api" => {
                    let _held = shared.api.lock().unwrap();
                    read(&shared, now)
                }
                _ => {
                    let _held = shared.health.lock().unwrap();
                    read(&shared, now)
                }
            };
            skipped.is_none()
        };
        for what in ["config", "resources", "api", "health"] {
            assert!(busy(what), "{what} busy");
        }
        assert!(read(&shared, now).is_some(), "free again");
    }

    #[test]
    fn open_handoffs_singular_plural_thousands_and_unknown() {
        let now = Utc::now();
        let base = || MenuReading::from_snapshot(&resources(now), "ai-memory", 2, now);
        let config = Config {
            api_seconds: 60,
            ..Default::default()
        };
        let with = |api: Option<&ApiSnapshot>| {
            label_of(&base().with_context(api, None, &config, now), "sessions")
        };
        for (n, want) in [
            (0, "0 handoffs abertos"),
            (1, "1 handoff aberto"),
            (2, "2 handoffs abertos"),
            (1234, "1.234 handoffs abertos"),
        ] {
            assert_eq!(with(Some(&api(now, n))), want, "{n}");
        }
        let unknown = "Handoffs abertos: —";
        assert_eq!(with(None), unknown, "no API reading");
        assert_eq!(
            with(Some(&ApiSnapshot::default())),
            unknown,
            "before the 1st"
        );
        let no_total = ApiSnapshot {
            at: Some(now),
            ..Default::default()
        };
        assert_eq!(with(Some(&no_total)), unknown, "no total from the server");
        // Três intervalos (180 s) ainda valem; 181 s, não.
        let at = |secs| ApiSnapshot {
            at: Some(now - Duration::seconds(secs)),
            handoff_total: Some(5),
            ..Default::default()
        };
        assert_eq!(with(Some(&at(180))), "5 handoffs abertos");
        assert_eq!(with(Some(&at(181))), unknown, "stale");
    }

    #[test]
    fn pending_proposals_at_the_limits_and_without_a_project_or_a_reading() {
        let now = Utc::now();
        let base = || MenuReading::from_snapshot(&resources(now), "ai-memory", 2, now);
        let pending = |n: Option<usize>, age: i64| crate::health::Snapshot {
            at: Some(now - Duration::seconds(age)),
            pending: n,
            pending_scope: Some(scope()),
            ..Default::default()
        };
        let watched = |text: &str| format!("{text} · acme / api-pagamentos");
        for (case, health, config, want) in [
            (
                "zero",
                Some(pending(Some(0), 0)),
                watching(),
                watched("0 propostas pendentes"),
            ),
            (
                "one",
                Some(pending(Some(1), 0)),
                watching(),
                watched("1 proposta pendente"),
            ),
            (
                "two",
                Some(pending(Some(2), 0)),
                watching(),
                watched("2 propostas pendentes"),
            ),
            (
                "199",
                Some(pending(Some(199), 0)),
                watching(),
                watched("199 propostas pendentes"),
            ),
            (
                "the list limit",
                Some(pending(Some(200), 0)),
                watching(),
                watched("200+ propostas pendentes"),
            ),
            (
                "past the limit",
                Some(pending(Some(201), 0)),
                watching(),
                watched("200+ propostas pendentes"),
            ),
            (
                "denied is unknown",
                Some(pending(None, 0)),
                watching(),
                watched("Propostas: —"),
            ),
            (
                "stale is unknown",
                Some(pending(Some(3), 91)),
                watching(),
                watched("Propostas: —"),
            ),
            (
                "no project",
                Some(pending(Some(3), 0)),
                Config::default(),
                "Propostas: selecione um projeto em Ajustes".into(),
            ),
            (
                "no health reading",
                None,
                watching(),
                "Propostas: aguardando".into(),
            ),
        ] {
            let reading = base().with_context(None, health.as_ref(), &config, now);
            assert_eq!(label_of(&reading, "review"), want, "{case}");
        }
    }

    #[test]
    fn the_alert_band_shows_only_with_a_fresh_health_reading_with_alerts() {
        let now = Utc::now();
        let failed = serde_json::json!({"consolidation": {"failed": 1}, "write_queue": [9, 10]});
        let stale = crate::health::Snapshot {
            at: Some(now - Duration::seconds(crate::health::INTERVAL as i64 * 3 + 1)),
            ..health(now, failed.clone())
        };
        // (caso, saúde, visível, rótulo)
        for (case, health, visible, label) in [
            (
                "fresh with alerts",
                Some(health(now, failed.clone())),
                true,
                "Saúde: Consolidações com falha · Fila de escrita acima de 80%",
            ),
            (
                "fresh without alerts",
                Some(health(
                    now,
                    serde_json::json!({"consolidation": {"failed": 0}}),
                )),
                false,
                "",
            ),
            ("stale with alerts", Some(stale), false, ""),
            ("no reading", None, false, ""),
        ] {
            let reading = MenuReading::from_snapshot(&resources(now), "ai-memory", 2, now)
                .with_context(None, health.as_ref(), &watching(), now);
            let band = &flats(&reading)[3];
            assert_eq!(
                (band.visible, band.label.as_str()),
                (visible, label),
                "{case}"
            );
            assert!(!band.enabled && !band.separator, "{case}");
        }
    }

    #[test]
    fn the_alert_dot_needs_alerts_and_the_option() {
        let now = Utc::now();
        let alerts = serde_json::json!({"consolidation": {"failed": 1}});
        let quiet = serde_json::json!({"consolidation": {"failed": 0}});
        for (alert, option, want) in [
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (true, true, true),
        ] {
            let config = Config {
                alert_dot: option,
                ..Default::default()
            };
            let status = if alert { alerts.clone() } else { quiet.clone() };
            let reading = MenuReading::from_snapshot(&resources(now), "ai-memory", 2, now)
                .with_context(None, Some(&health(now, status)), &config, now);
            assert_eq!(reading.alert_dot, want, "alert {alert} option {option}");
            // A faixa do menu não depende da opção do ponto.
            assert_eq!(
                flats(&reading)[3].visible,
                alert,
                "alert {alert} option {option}"
            );
        }
        // Saúde velha com alertas: nem ponto.
        let old = crate::health::Snapshot {
            at: Some(now - Duration::seconds(crate::health::INTERVAL as i64 * 3 + 1)),
            ..health(now, alerts)
        };
        let reading = MenuReading::from_snapshot(&resources(now), "ai-memory", 2, now)
            .with_context(None, Some(&old), &Config::default(), now);
        assert!(!reading.alert_dot);
    }

    #[test]
    fn the_bar_title_of_each_mode_for_fresh_zero_unavailable_and_late_data() {
        let now = Utc::now();
        let reading = |ai, docker| MenuReading {
            ai,
            docker,
            ..MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, now)
        };
        let load = |cpu, memory| Some(Load { cpu, memory });
        let mut late = resources(now);
        late.at = Some(now - Duration::seconds(11));
        let late = MenuReading::from_snapshot(&late, "ai-memory", 2, now);
        // (caso, leitura, [Só o ícone, Ícone + CPU, Completo]) com
        // (texto, esmaecido).
        for (case, r, want) in [
            (
                "fresh",
                reading(load(12.4, 421 * MIB), load(15.4, 733 * MIB)),
                [
                    ("", false),
                    ("12%", false),
                    ("AI 12% 421M | Docker 15% 733M", false),
                ],
            ),
            (
                "zero",
                reading(load(0., 0), load(0., 0)),
                [
                    ("", false),
                    ("0%", false),
                    ("AI 0% 0M | Docker 0% 0M", false),
                ],
            ),
            (
                "unavailable",
                reading(None, None),
                [("", true), ("—", true), ("AI — | Docker —", true)],
            ),
            (
                "only Docker unavailable",
                reading(load(12.4, 421 * MIB), None),
                [
                    ("", false),
                    ("12%", false),
                    ("AI 12% 421M | Docker —", true),
                ],
            ),
            (
                "only ai-memory unavailable",
                reading(None, load(15.4, 733 * MIB)),
                [("", true), ("—", true), ("AI — | Docker 15% 733M", true)],
            ),
            (
                "late",
                late,
                [("", true), ("—", true), ("AI — | Docker —", true)],
            ),
            (
                "GiB and thousands",
                reading(load(1234.56, 3 << 29), load(2000.4, (1 << 30) - 1)),
                [
                    ("", false),
                    ("1.235%", false),
                    ("AI 1.235% 1,5G | Docker 2.000% 1.024M", false),
                ],
            ),
        ] {
            for (mode, (text, dimmed)) in [BarMode::Icon, BarMode::IconCpu, BarMode::Full]
                .into_iter()
                .zip(want)
            {
                // Um modo novo tem de aparecer nesta matriz.
                match mode {
                    BarMode::Icon | BarMode::IconCpu | BarMode::Full => {}
                }
                assert_eq!(
                    bar_title(&r, mode),
                    BarTitle {
                        text: text.into(),
                        dimmed
                    },
                    "{case} {mode:?}"
                );
            }
        }
    }

    #[test]
    fn compact_bytes_switches_unit_at_one_gib_with_pt_br_separators() {
        assert_eq!(compact_bytes((1 << 30) - 1), "1.024M");
        assert_eq!(compact_bytes(1 << 30), "1,0G");
        assert_eq!(compact_bytes(0), "0M");
        assert_eq!(compact_bytes(3 << 29), "1,5G");
    }

    #[test]
    fn the_tooltip_keeps_the_details_and_the_captions_the_menu_left_out() {
        let now = Utc::now();
        let reading = full(now);
        assert_eq!(
            tooltip(&reading),
            [
                "AI Monitor · ao vivo · há 1 s",
                "ai-memory · 12,4% · 421 MiB",
                "Docker · 3 ativos · 15,5% · 733 MiB",
                "75 handoffs abertos",
                "3 propostas pendentes · acme / api-pagamentos",
                "Saúde: Consolidações com falha",
                "CPU: 100% = 1 núcleo · M/G = MiB/GiB",
                "Docker = contêineres ativos, incluindo ai-memory",
            ]
            .join("\n")
        );
        // As legendas saíram do menu.
        for f in flats(&reading) {
            assert!(
                !f.label.contains("núcleo") && !f.label.contains("incluindo"),
                "{f:?}"
            );
        }
    }

    #[test]
    fn print_menu_is_the_flat_menu_of_a_reading_without_data() {
        for (config, proposals) in [
            (
                Config::default(),
                "Propostas: selecione um projeto em Ajustes",
            ),
            (watching(), "Propostas: — · acme / api-pagamentos"),
        ] {
            let json: serde_json::Value = serde_json::from_str(&print_menu(&config)).unwrap();
            let rows = json.as_array().expect("an array");
            let now = Utc::now();
            let empty = MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, now)
                .with_context(
                    Some(&Default::default()),
                    Some(&Default::default()),
                    &config,
                    now,
                );
            assert_eq!(
                *rows,
                flats(&empty)
                    .into_iter()
                    .map(|f| serde_json::to_value(f).unwrap())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                rows[0],
                serde_json::json!({
                    "label": "AI Monitor · aguardando a primeira coleta",
                    "enabled": false, "separator": false, "dynamic": true,
                    "visible": true, "action": null
                })
            );
            assert_eq!(rows[6]["label"], proposals);
            assert_eq!(rows[3]["visible"], false, "no alert band without data");
        }
    }
}
