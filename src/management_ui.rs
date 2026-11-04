//! Revisão (Propostas, Handoffs) e Manutenção e backup (Manutenção, Backup),
//! as duas páginas do mesmo painel (Review.dc.html).
//!
//! Regras desta tela:
//! - Linha de ferramentas: o projeto, o segmentado das abas da página e, à
//!   direita, as ações da aba (Propostas: Estado e Atualizar; Handoffs:
//!   histórico e expiração). Sem espaço, as ações descem para a linha seguinte.
//! - Propostas: a lista tem consulta, voo e resposta próprios (`List`). Pede
//!   a lista (a GET de até 200) ao escolher o projeto ou o estado, ao abrir a
//!   aba com projeto e em Atualizar; resposta de uma consulta anterior é
//!   ignorada. A lista não toca no recibo nem no resultado, e eles não tocam
//!   nela.
//! - O detalhe da proposta é a GET `/admin/pending-writes/{id}` da seleção,
//!   com consulta, voo e resposta próprios (`Detail`): uma por seleção (um
//!   clique na linha, ou o ID digitado ao sair do campo); a resposta de uma
//!   seleção anterior é ignorada. Não prende o painel, e a lista, o recibo e
//!   o resultado não o apagam (nem ele a eles); trocar de aba ou de projeto,
//!   sim.
//! - Atualizar relê a lista e a proposta selecionada; Aprovar e Rejeitar
//!   executados com sucesso também (só leitura). Com falha, nada é relido.
//! - Propostas: o campo de ID (com "Ver detalhes") fica no pé da coluna da
//!   lista; Rejeitar… e Aprovar…, na linha do título do detalhe, à direita, e
//!   o motivo da rejeição no pé do detalhe. Nada disso se move quando o
//!   detalhe chega: a linha de estado tem a altura reservada e o corpo rola
//!   entre as duas. Rejeitar… e Aprovar… desabilitam no lugar, com a dica,
//!   quando o estado conhecido da proposta (o do detalhe ou, enquanto ele não
//!   chega, o da linha da lista) não é pendente; desconhecido, ficam
//!   habilitados e a prévia decide.
//! - Handoffs: a lista são os handoffs abertos do projeto na coleta do monitor
//!   (`ApiSnapshot`); nenhuma consulta nova.
//! - A seleção é o campo de ID: clicar numa linha o preenche; um ID fora da
//!   lista continua valendo. Largura de conteúdo ≥ `SIDE_BY_SIDE`: lista e
//!   detalhe lado a lado; abaixo, um de cada vez, com "← Voltar". Na tela
//!   estreita, Propostas abre o detalhe com um clique na linha ou com Enter
//!   no campo de ID; Handoffs, com "Informar ID…".
//! - Um resultado, um erro ou o aviso de prévia vencida ficam na página até
//!   "Limpar", trocar de aba ou de projeto; uma operação em andamento prende o
//!   painel até a resposta.
//! - A folha de confirmação trava o que descartaria a prévia ou mexeria
//!   atrás dela: o projeto, as abas e as ações das duas abas (as de
//!   Propostas, Atualizar e Estado, só a folha trava: a lista tem voo
//!   próprio). O véu só engole cliques; sem a trava, o teclado (Tab e Enter)
//!   chegaria a eles.
use crate::{
    client::{Error, Scope},
    confirmation::Confirmation,
    console, handoff_text,
    maintenance::{Action, Intent, Read, Receipt, Task, Work, Worker},
    model::{ApiSnapshot, Handoff, Project, number},
    monitor::Shared,
    sessions_ui::{age, dated, headline},
    theme,
    widgets::{self, Answer, Button, Fact, Segment, Tone, Type, space},
};
use chrono::{DateTime, Utc};
use eframe::egui::{
    self, Align, FontId, Label, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, WidgetInfo,
    WidgetType, pos2, vec2,
};
use serde_json::Value;
use std::sync::{Arc, atomic::Ordering};

/// Largura de conteúdo a partir da qual lista e detalhe ficam lado a lado:
/// 320 da lista + 1 da divisória + 400 do detalhe (as bases do artboard).
const SIDE_BY_SIDE: f32 = 721.;
const LIST_MIN: f32 = 320.;
const LIST_MAX: f32 = 380.;
const ROW_HEIGHT: f32 = 56.;
/// O pé da lista estreita ("Informar ID…").
const LIST_FOOT: f32 = 40.;
/// Folga à direita do conteúdo rolável do detalhe e do resultado, para a
/// barra de rolagem.
const DETAIL_GAP: f32 = space::L;
/// Largura máxima do seletor de estado, e a do de projeto quando falta
/// espaço na linha; o nome mais longo corta.
const PROJECT_WIDTH: f32 = 260.;
/// Largura máxima dos campos dos formulários e do detalhe.
const FIELD: f32 = 420.;
/// Coluna dos rótulos nas linhas rótulo/campo dos cartões.
const LABEL: f32 = 200.;
/// Largura máxima dos cartões de Manutenção e backup.
const CARD: f32 = 640.;
/// Altura máxima do resultado; o resto rola.
const RESULT_HEIGHT: f32 = 240.;
/// Propostas por consulta (o `limit` da GET).
const LIMIT: usize = 200;
/// Linhas iguais mantidas em volta de cada mudança da prévia.
const CONTEXT: usize = 3;
/// Acima de tantas comparações de linha, a prévia não procura o menor diff:
/// mostra o trecho diferente inteiro como removido e acrescentado.
const DIFF_CELLS: usize = 250_000;
const REVIEW_NOTE: &str =
    "Até 200 propostas por estado · operações dependem das permissões do servidor";
const HANDOFF_NOTE: &str = "Handoffs abertos na última coleta do monitor · até 200 por projeto · operações dependem das permissões do servidor";
const MAINTENANCE_NOTE: &str = "Revise o escopo e a prévia antes de confirmar. Operações dependem das permissões e recursos habilitados no servidor.";
/// A dica de Rejeitar… e Aprovar… desabilitados por estado.
const NOT_PENDING: &str = "Só propostas pendentes podem ser aprovadas ou rejeitadas.";
/// Estados das propostas: (valor da API, plural do filtro, singular do detalhe).
const STATUSES: [(&str, &str, &str); 5] = [
    ("pending", "pendentes", "pendente"),
    ("approved", "aprovadas", "aprovada"),
    ("rejected", "rejeitadas", "rejeitada"),
    ("conflict", "em conflito", "em conflito"),
    ("failed", "com falha", "com falha"),
];

/// As duas páginas que compartilham este painel (e o seu estado): Revisão
/// (Propostas, Handoffs) e Manutenção e backup (Manutenção, Backup).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Section {
    Review,
    Maintenance,
}
impl Section {
    const ALL: [Section; 2] = [Section::Review, Section::Maintenance];
    pub fn label(self) -> &'static str {
        match self {
            Section::Review => "Revisão",
            Section::Maintenance => "Manutenção e backup",
        }
    }
    /// Abas da página; a primeira é a que abre.
    fn tabs(self) -> [(u8, &'static str); 2] {
        match self {
            Section::Review => [(0, "Propostas"), (1, "Handoffs")],
            Section::Maintenance => [(2, "Manutenção"), (3, "Backup")],
        }
    }
    /// A legenda sem projeto escolhido; backup e checkpoints são da
    /// Manutenção e backup.
    fn pick_project(self) -> &'static str {
        match self {
            Section::Review => "Selecione o projeto para continuar.",
            Section::Maintenance => {
                "Selecione o projeto para continuar. Backup e checkpoints indicam explicitamente quando abrangem toda a instalação."
            }
        }
    }
    /// A página cujas `tabs` têm `tab`.
    fn of(tab: u8) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.tabs().iter().any(|&(id, _)| id == tab))
            .unwrap_or(Section::Maintenance)
    }
}

/// O que a linha de ferramentas pediu neste frame.
#[derive(Clone, Copy, PartialEq)]
enum Ask {
    Refresh,
    History,
    Expire,
}

/// Uma leitura de Revisão · Propostas com consulta, voo e resposta próprios,
/// num trabalhador só dela: a lista e o detalhe da proposta selecionada. Não
/// toca no recibo, no resultado nem na outra leitura, e eles não tocam nela.
struct Fetch<T> {
    worker: Worker,
    /// A leitura de uma chave (projeto, estado ou ID).
    read: fn(Scope, String) -> Read,
    /// A resposta no formato da tela; `Err` = formato incompatível.
    parse: fn(Value) -> Result<T, Error>,
    /// A senha da consulta mais recente: resposta com outra é de uma chave
    /// anterior e é ignorada.
    ticket: u64,
    /// A chave da consulta mais recente.
    key: Option<(Scope, String)>,
    /// `None` = carregando (ou nada pedido).
    value: Option<Result<T, String>>,
    /// A consulta mais recente ainda não entrou na fila do trabalhador
    /// (ocupado com as anteriores); `poll` tenta de novo.
    unsent: bool,
}
impl<T> Fetch<T> {
    fn new(
        shared: Arc<Shared>,
        read: fn(Scope, String) -> Read,
        parse: fn(Value) -> Result<T, Error>,
    ) -> Self {
        Self {
            worker: Worker::new(shared),
            read,
            parse,
            ticket: 0,
            key: None,
            value: None,
            unsent: false,
        }
    }
    fn reset(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.key = None;
        self.value = None;
        self.unsent = false;
    }
    fn request(&mut self, shared: &Shared, key: (Scope, String)) {
        self.ticket = self.ticket.wrapping_add(1);
        self.key = Some(key);
        self.value = None;
        self.unsent = true;
        self.send(shared);
    }
    fn send(&mut self, shared: &Shared) {
        let Some((scope, key)) = self.key.clone().filter(|_| self.unsent) else {
            return;
        };
        self.unsent = self
            .worker
            .submit(self.ticket, shared, Work::Read((self.read)(scope, key)))
            .is_err();
    }
    fn poll(&mut self, shared: &Shared) {
        while let Ok(reply) = self.worker.replies.try_recv() {
            if reply.ticket != self.ticket
                || reply.generation != shared.generation.load(Ordering::Acquire)
            {
                continue;
            }
            self.value = Some(reply.result.and_then(self.parse).map_err(|e| e.to_string()));
        }
        self.send(shared);
    }
}
/// A lista: a GET de até `LIMIT` propostas do projeto e do estado.
type List = Fetch<Vec<Value>>;
impl List {
    fn proposals(shared: Arc<Shared>) -> Self {
        Self::new(
            shared,
            |scope, status| Read::Proposals { scope, status },
            |value| match value {
                Value::Array(mut rows) => {
                    rows.truncate(LIMIT);
                    Ok(rows)
                }
                _ => Err(Error::Format),
            },
        )
    }
    /// As linhas carregadas para `key`; vazio enquanto carrega ou com erro.
    fn loaded(&self, key: &(Scope, String)) -> Option<&[Value]> {
        match &self.value {
            Some(Ok(rows)) if self.key.as_ref() == Some(key) => Some(rows),
            _ => None,
        }
    }
}
/// O detalhe: a GET `/admin/pending-writes/{id}` da proposta selecionada.
type Detail = Fetch<Value>;
impl Detail {
    fn proposal(shared: Arc<Shared>) -> Self {
        Self::new(shared, |scope, id| Read::Proposal { scope, id }, Ok)
    }
}

pub struct Management {
    worker: Worker,
    pub busy: bool,
    ticket: u64,
    scope: Option<Scope>,
    tab: u8,
    result: Option<Value>,
    receipt: Option<Receipt>,
    /// O diff da prévia de Aprovar ou Rejeitar do recibo: calculado uma vez,
    /// quando o recibo chega (`poll`), e descartado com ele (`clear`).
    proposed: Option<Proposed>,
    error: Option<String>,
    notice: String,
    /// A prévia venceu e `poll` a descartou; só resta o aviso em `notice`.
    /// Prende o painel como um resultado na tela: sai com um clique.
    expired: bool,
    confirm: String,
    status: String,
    identifier: String,
    reason: String,
    task: Task,
    reembed: bool,
    days: u32,
    session: String,
    path: String,
    signal: String,
    version: String,
    git: bool,
    message: String,
    destination: String,
    okf: bool,
    list: List,
    detail: Detail,
    /// O campo de ID da proposta tinha o foco no último frame: o ID ainda
    /// está sendo digitado e não pede o detalhe.
    editing: bool,
    /// A operação em voo é Aprovar ou Rejeitar: com sucesso, `refresh`.
    reread: bool,
    /// Tela estreita: o detalhe no lugar da lista.
    open: bool,
    /// O frame em que a aba Propostas foi mostrada pela última vez: um
    /// intervalo maior que um frame é uma abertura, que pede a lista.
    seen: Option<u64>,
}
impl Management {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            worker: Worker::new(shared.clone()),
            busy: false,
            ticket: 0,
            scope: None,
            tab: 0,
            result: None,
            receipt: None,
            proposed: None,
            error: None,
            notice: String::new(),
            expired: false,
            confirm: String::new(),
            status: "pending".into(),
            identifier: String::new(),
            reason: String::new(),
            task: Task::Lint,
            reembed: false,
            days: 7,
            session: String::new(),
            path: String::new(),
            signal: "helpful".into(),
            version: String::new(),
            git: false,
            message: "Checkpoint solicitado pelo AI Monitor".into(),
            destination: String::new(),
            okf: false,
            list: List::proposals(shared.clone()),
            detail: Detail::proposal(shared),
            editing: false,
            reread: false,
            open: false,
            seen: None,
        }
    }
    pub fn reset(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.scope = None;
        self.clear();
        self.identifier.clear();
        self.reason.clear();
        self.session.clear();
        self.path.clear();
        self.version.clear();
        self.destination.clear();
        self.list.reset();
        self.detail.reset();
        self.open = false;
        self.seen = None;
    }
    fn clear(&mut self) {
        self.receipt = None;
        self.proposed = None;
        self.result = None;
        self.error = None;
        self.confirm.clear();
        self.notice.clear();
        self.expired = false;
    }
    /// Troca de aba ou de projeto: descarta prévia, resultado, os campos e
    /// o detalhe da proposta.
    fn discard(&mut self) {
        self.clear();
        self.detail.reset();
        self.identifier.clear();
        self.reason.clear();
        self.path.clear();
        self.version.clear();
        self.session.clear();
        self.open = false;
    }
    /// O que a troca de aba descartaria e o usuário ainda não viu ou
    /// decidiu: operação em andamento, resultado ou erro na tela, ou o aviso
    /// de que a prévia expirou. Uma prévia aguardando confirmação sempre vem
    /// com o seu resultado.
    fn holds(&self) -> bool {
        self.busy || self.result.is_some() || self.error.is_some() || self.expired
    }
    /// `section` aberta com o painel preso à outra página: avisa, oferece ir
    /// até lá e, sem operação em voo, descartar. Nada muda sem um clique.
    /// Devolve a página pedida pelo "Ir para".
    fn elsewhere(&mut self, ui: &mut Ui, section: Section) -> Option<Section> {
        let owner = Section::of(self.tab);
        let tab = owner
            .tabs()
            .into_iter()
            .find(|&(id, _)| id == self.tab)
            .map_or("", |(_, label)| label);
        let place = format!("{} · {tab}", owner.label());
        let (bold, text) = if self.busy {
            (
                format!("Operação em andamento em {place}."),
                "Aguarde a resposta lá; esta página usa o mesmo painel.",
            )
        } else if self.receipt.is_some() {
            (
                format!("Prévia aguardando confirmação em {place}."),
                "Confirme-a lá ou descarte-a para usar esta página.",
            )
        } else if self.expired {
            (
                format!("Prévia expirada em {place}."),
                "Veja o aviso lá ou descarte-o para usar esta página.",
            )
        } else if self.error.is_some() {
            (
                format!("Erro ainda na tela em {place}."),
                "Confira-o lá ou descarte-o para usar esta página.",
            )
        } else {
            (
                format!("Resultado ainda na tela em {place}."),
                "Confira-o lá ou descarte-o para usar esta página.",
            )
        };
        let go = widgets::strip(
            ui,
            Tone::Warn,
            &bold,
            text,
            Some(&format!("Ir para {}", owner.label())),
        )
        .is_some_and(|r| r.clicked());
        if !self.busy
            && ui
                .add(widgets::Button::secondary(&format!(
                    "Descartar e abrir {}",
                    section.label()
                )))
                .clicked()
        {
            self.discard();
            self.tab = section.tabs()[0].0;
        }
        go.then_some(owner)
    }
    fn begin(&mut self, shared: &Shared, work: Work) {
        if self.busy {
            return;
        }
        self.ticket = self.ticket.wrapping_add(1);
        self.clear();
        match self.worker.submit(self.ticket, shared, work) {
            Ok(()) => self.busy = true,
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn preview(&mut self, shared: &Shared, scope: &Scope, intent: Intent) {
        self.begin(
            shared,
            Work::Preview(Action {
                scope: scope.clone(),
                intent,
            }),
        );
    }
    pub fn poll(&mut self, shared: &Shared) {
        self.list.poll(shared);
        self.detail.poll(shared);
        let mut refresh = false;
        while let Ok(reply) = self.worker.replies.try_recv() {
            self.busy = false;
            let reread = std::mem::take(&mut self.reread);
            if reply.ticket != self.ticket
                || reply.generation != shared.generation.load(Ordering::Acquire)
            {
                continue;
            }
            match reply.result {
                Ok(v) => {
                    refresh |= reread;
                    // O diff sai uma vez, aqui; a folha só o mostra.
                    self.proposed = reply
                        .receipt
                        .as_ref()
                        .and_then(|r| proposed(&r.action.intent, &v));
                    self.result = Some(v);
                    self.receipt = reply.receipt;
                    self.notice = if reply.applying {
                        "Resposta recebida. Confira os detalhes abaixo."
                    } else if self.receipt.is_some() {
                        // A prévia abre na folha, que já é o aviso.
                        ""
                    } else {
                        "Consulta concluída; revise os dados abaixo."
                    }
                    .into();
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    if reply.applying {
                        self.notice="A solicitação pode ter sido processada mesmo sem uma resposta válida. Confira o estado no servidor antes de tentar novamente. Não houve repetição automática.".into();
                    }
                }
            }
        }
        if refresh {
            self.refresh(shared);
        }
        if self.receipt.as_ref().is_some_and(Receipt::expired) {
            self.clear();
            self.notice = "A prévia expirou após 5 minutos. Prepare a operação novamente.".into();
            self.expired = true;
        }
    }
    /// A página `section`. Se o painel está em aba da outra página, troca
    /// para a primeira aba desta, a menos que isso descarte algo (`holds`):
    /// aí mostra o aviso e mantém a aba. Devolve a página pedida pelo aviso.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        shared: &Arc<Shared>,
        api: &ApiSnapshot,
        section: Section,
        now: DateTime<Utc>,
    ) -> Option<Section> {
        self.poll(shared);
        if Section::of(self.tab) != section {
            if self.holds() {
                return self.elsewhere(ui, section);
            }
            self.tab = section.tabs()[0].0;
            self.discard();
        }
        let before = (self.scope.clone(), self.tab, self.status.clone());
        let ask = self.toolbar(ui, api, section);
        if (&before.0, before.1) != (&self.scope, self.tab) {
            self.discard();
        } else if before.2 != self.status {
            // Outro estado: a tela estreita volta à lista.
            self.open = false;
        }
        let Some(scope) = self.scope.clone() else {
            widgets::caption(ui, section.pick_project());
            return None;
        };
        match ask {
            Some(Ask::History) => self.begin(shared, Work::Read(Read::Handoffs(scope.clone()))),
            Some(Ask::Expire) => {
                self.preview(shared, &scope, Intent::ExpireHandoffs { days: self.days })
            }
            Some(Ask::Refresh) | None => {}
        }
        if self.tab == 0 {
            let frame = ui.ctx().cumulative_frame_nr();
            let opened = self.seen.is_none_or(|seen| frame > seen + 1);
            self.seen = Some(frame);
            let key = (scope.clone(), self.status.clone());
            if ask == Some(Ask::Refresh) {
                self.refresh(shared);
            } else if opened || self.list.key.as_ref() != Some(&key) {
                self.list.request(shared, key);
            }
        }
        self.outcome(ui);
        ui.add_enabled_ui(!self.busy && self.receipt.is_none(), |ui| match self.tab {
            0 => self.proposals(ui, shared, &scope, now),
            1 => self.handoffs(ui, shared, &scope, api, now),
            2 => self.maintenance(ui, shared, &scope),
            _ => self.backups(ui, shared, &scope),
        });
        self.sheet(ui, shared);
        None
    }
    /// A linha de ferramentas: o projeto e as abas à esquerda; à direita, as
    /// ações da aba (só com projeto), que descem quando não cabem.
    fn toolbar(&mut self, ui: &mut Ui, api: &ApiSnapshot, section: Section) -> Option<Ask> {
        let projects: Vec<(Option<Scope>, String)> = api
            .projects
            .iter()
            .map(|p| {
                let s = Scope {
                    workspace: p.workspace_name.clone(),
                    project: p.project_name.clone(),
                };
                let label = s.label();
                (Some(s), label)
            })
            .collect();
        let shown = self
            .scope
            .as_ref()
            .map_or_else(|| "Selecione um projeto".to_owned(), Scope::label);
        let counts = self.counts(api);
        let tabs = section.tabs();
        let items: Vec<Segment<'_, u8>> = tabs
            .iter()
            .zip(counts)
            .map(|(&(value, label), count)| Segment {
                value,
                label,
                count,
            })
            .collect();
        // O seletor de projeto alarga até o que sobra na linha. As ações
        // ficam ao lado quando cabem com ele em `PROJECT_WIDTH` (senão
        // descem, como antes), e ele para antes delas; 1 de folga para o
        // arredondamento.
        let line =
            ui.available_width() - widgets::segmented_width(ui, &self.tab, &items) - 2. * space::S;
        let beside = actions_width(ui, self.tab, &self.status);
        let room = if self.scope.is_some()
            && line - picker_natural(ui, &shown).clamp(80., PROJECT_WIDTH) >= beside
        {
            line - beside - 1.
        } else {
            line + space::S - 1.
        };
        // A folha de confirmação também trava o que descartaria a prévia: o
        // véu só engole cliques, e o teclado (Tab e Enter) chegaria aqui.
        let (tab, locked, chosen) = (
            self.tab,
            self.busy || self.receipt.is_some(),
            self.scope.is_some(),
        );
        // As ações de Propostas só leem a lista, que tem voo próprio: a
        // operação em andamento não as trava, a folha sim (o teclado
        // releria ou trocaria a lista atrás do véu).
        let held = if tab == 0 {
            self.receipt.is_some()
        } else {
            locked
        };
        let (scope, current, status, days) = (
            &mut self.scope,
            &mut self.tab,
            &mut self.status,
            &mut self.days,
        );
        let mut below = false;
        let mut ask = widgets::toolbar(
            ui,
            |ui| {
                ui.add_enabled_ui(!locked, |ui| {
                    ui.spacing_mut().item_spacing.x = space::S;
                    picker(
                        ui,
                        "management-scope",
                        scope,
                        &shown,
                        room.max(PROJECT_WIDTH),
                        &projects,
                    );
                    widgets::segmented(ui, current, &items);
                });
            },
            |ui| {
                if !chosen {
                    return None;
                }
                if ui.available_width() < actions_width(ui, tab, status) {
                    below = true;
                    return None;
                }
                actions(ui, tab, status, days, held)
            },
        );
        if below {
            ask = ui
                .allocate_ui_with_layout(
                    vec2(ui.available_width(), 28.),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = space::S;
                        actions(ui, tab, &mut self.status, &mut self.days, held)
                    },
                )
                .inner;
        }
        ask
    }
    /// Os contadores do segmentado: só com projeto; "—" enquanto a lista
    /// carrega (ou falhou) ou antes da 1ª coleta.
    fn counts(&self, api: &ApiSnapshot) -> [Option<String>; 2] {
        let Some(scope) = &self.scope else {
            return [None, None];
        };
        match Section::of(self.tab) {
            Section::Review => {
                let key = (scope.clone(), self.status.clone());
                let proposals = self
                    .list
                    .loaded(&key)
                    .map_or_else(|| "—".to_owned(), |rows| number(rows.len()));
                let handoffs = if api.at.is_some() {
                    number(open_handoffs(api, scope).len())
                } else {
                    "—".into()
                };
                [Some(proposals), Some(handoffs)]
            }
            Section::Maintenance => [None, None],
        }
    }
    /// Operação em andamento, erro, prévia vencida ou resultado, no topo da
    /// aba; os três últimos saem com "Limpar".
    fn outcome(&mut self, ui: &mut Ui) {
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                widgets::caption(
                    ui,
                    "Operação em andamento. Aguarde a resposta antes de trocar a conexão.",
                );
            });
        }
        let clear = if let Some(e) = &self.error {
            widgets::strip(ui, Tone::Bad, e, &self.notice, Some("Limpar"))
                .is_some_and(|r| r.clicked())
        } else if self.expired {
            widgets::strip(ui, Tone::Warn, &self.notice, "", Some("Limpar"))
                .is_some_and(|r| r.clicked())
        } else if let (None, Some(result)) = (&self.receipt, &self.result) {
            // Com recibo, a prévia está na folha.
            let mut clear = false;
            widgets::card(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Resultado").font(Type::Section.font()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        clear = ui
                            .add(Button::secondary("Limpar").small().raised())
                            .clicked();
                    });
                });
                if !self.notice.is_empty() {
                    widgets::caption(ui, &self.notice);
                }
                widgets::scroll_bar(ui);
                egui::ScrollArea::vertical()
                    .id_salt("management-result")
                    .max_height(RESULT_HEIGHT)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width() - DETAIL_GAP);
                        console::show_values(ui, result, 0);
                    });
            });
            clear
        } else {
            false
        };
        if clear {
            self.clear();
        }
    }
    /// A folha de confirmação da prévia aguardando confirmação. O recibo é
    /// imutável: trocar de aba ou de projeto o cancela; os campos ficam
    /// travados até cancelar.
    fn sheet(&mut self, ui: &mut Ui, shared: &Shared) {
        let Some(receipt) = self.receipt.clone() else {
            return;
        };
        let action = &receipt.action;
        let proposed = self.proposed.as_ref();
        let mut facts = vec![Fact::new("Operação", action.label())];
        match &proposed {
            Some(p) => {
                facts.push(Fact::new("Alvo", p.target.as_str()).mono());
                if let Some((effect, reversible)) = p.consequences {
                    facts.push(Fact::new("Efeito", effect));
                    if let Some(reversible) = reversible {
                        facts.push(Fact::new("Reversível", reversible));
                    }
                }
            }
            None => facts.push(Fact::new("Alvo", action.target())),
        }
        // O motivo é opcional: em branco, a linha some.
        if let Intent::Reject { reason, .. } | Intent::Feedback { reason, .. } = &action.intent
            && !reason.trim().is_empty()
        {
            facts.push(Fact::new("Motivo", reason.as_str()));
        }
        let mut subtitle = format!("Projeto: {}", action.scope.label());
        if let Intent::Consolidate { .. } = &action.intent {
            subtitle.push_str(". O projeto selecionado é apenas contexto: esta operação é identificada pela sessão global.");
        }
        let typed = match action.confirmation() {
            Some(Confirmation::Type(target)) => Some(target),
            // Leitura não gera recibo.
            Some(Confirmation::Click) | None => None,
        };
        // Leitura não gera recibo.
        let (title, verb) = action.question().unwrap_or_default();
        let result = &self.result;
        let answer = widgets::confirm_sheet(
            ui,
            &widgets::Confirm {
                title: &title,
                subtitle: &subtitle,
                facts: &facts,
                preview: "Prévia da alteração",
                warning: action.warning(),
                typed: typed.as_deref(),
                verb,
                left: receipt.left(),
                ready: !self.busy,
            },
            &mut self.confirm,
            |ui| match (&proposed, result) {
                (Some(p), _) => diff_view(ui, &p.lines),
                (None, Some(result)) => console::show_values(ui, result, 0),
                (None, None) => {}
            },
        );
        match answer {
            Some(Answer::Confirm) => {
                let changes = matches!(
                    receipt.action.intent,
                    Intent::Approve { .. } | Intent::Reject { .. }
                );
                self.begin(shared, Work::Apply(receipt));
                // Só o que entrou em voo é relido na resposta.
                self.reread = self.busy && changes;
            }
            Some(Answer::Cancel) => self.clear(),
            None => {}
        }
    }
    /// Relê a lista e a proposta selecionada: Atualizar, e Aprovar ou
    /// Rejeitar executados com sucesso. `Fetch::request` troca a senha: uma
    /// resposta anterior ainda em voo é ignorada e não sobrescreve a nova.
    fn refresh(&mut self, shared: &Shared) {
        if let Some(scope) = self.scope.clone() {
            self.list.request(shared, (scope, self.status.clone()));
        }
        if let Some(key) = self.detail.key.clone() {
            self.detail.request(shared, key);
        }
    }
    /// "Gerenciar em Revisão →" (Sessões e handoffs): Revisão · Handoffs com
    /// o projeto `scope` e o handoff `id`, a menos que a troca descarte algo
    /// (`holds`); aí nada muda e a página Revisão mostra o que prende o
    /// painel. Um projeto fora da lista de projetos fica sem escolha.
    pub fn open_handoff(&mut self, projects: &[Project], scope: &str, id: &str) {
        if self.holds() {
            return;
        }
        let scope = projects
            .iter()
            .map(|p| Scope {
                workspace: p.workspace_name.clone(),
                project: p.project_name.clone(),
            })
            .find(|s| s.label() == scope);
        if self.tab != 1 || self.scope != scope {
            self.discard();
        }
        self.tab = 1;
        if scope.is_some() {
            self.identifier = id.into();
            self.open = true;
        }
        self.scope = scope;
    }
    #[cfg(test)]
    pub(crate) fn tab(&self) -> u8 {
        self.tab
    }
    #[cfg(test)]
    pub(crate) fn set_tab(&mut self, tab: u8) {
        self.tab = tab;
    }
    /// O projeto e o ID escolhidos.
    #[cfg(test)]
    pub(crate) fn picked(&self) -> (Option<String>, &str) {
        (self.scope.as_ref().map(Scope::label), &self.identifier)
    }
    /// Uma prévia aguardando confirmação, como `poll` a deixaria.
    #[cfg(test)]
    pub(crate) fn hold_preview(&mut self, receipt: Receipt, result: Value) {
        self.scope = Some(receipt.action.scope.clone());
        self.proposed = proposed(&receipt.action.intent, &result);
        self.receipt = Some(receipt);
        self.result = Some(result);
    }
    #[cfg(test)]
    pub(crate) fn has_receipt(&self) -> bool {
        self.receipt.is_some()
    }
    fn proposals(&mut self, ui: &mut Ui, shared: &Shared, s: &Scope, now: DateTime<Utc>) {
        let key = (s.clone(), self.status.clone());
        let singular = status_label(&self.status).2.to_owned();
        let mut ask = None;
        widgets::split(ui, &[REVIEW_NOTE.into()], |ui| {
            let rect = ui.max_rect();
            let wide = rect.width() >= SIDE_BY_SIDE;
            if !wide && self.open {
                // O campo de ID fica na lista: nada está sendo digitado.
                self.editing = false;
                self.select(shared, s);
                ask = self.proposal(ui, s, rect, true, now);
                return;
            }
            let column = list_rect(rect, wide);
            // O pé antes da lista: o campo guarda o id (e o foco) quando ela
            // muda.
            let (foot, entered) = self.id_foot(ui, column, &mut ask);
            let list = Rect::from_min_max(column.min, pos2(column.right(), foot.top()));
            let mut area = ui.new_child(UiBuilder::new().max_rect(list));
            // O landmark da lista, na mesma área em todo estado (linhas,
            // vazia, carregando ou erro).
            widgets::pane(&mut area, list, "Lista de propostas");
            let picked = match (&self.list.value, self.list.loaded(&key)) {
                (_, Some([])) => {
                    empty_in(
                        &mut area,
                        list,
                        &format!("Nenhuma proposta {singular}"),
                        "Nenhum resultado visível para este estado e as permissões atuais.",
                    );
                    None
                }
                (_, Some(rows)) => rows_view(
                    &mut area,
                    list,
                    "proposals",
                    rows.len(),
                    false,
                    |ui, rect, i| {
                        let row = &rows[i];
                        let id = row["id"].as_str().unwrap_or_default();
                        let title = row["title"].as_str().unwrap_or("Proposta");
                        let second = row["target_path"].as_str().unwrap_or("—");
                        let when =
                            proposal_age(row, now).map_or_else(|| "—".to_owned(), |a| a.short());
                        let picked = !id.is_empty() && id == self.identifier.trim();
                        row_view(ui, rect, i, (title, second, &when), true, picked)
                            .clicked()
                            .then(|| id.to_owned())
                    },
                ),
                (Some(Err(e)), None) => {
                    empty_in(
                        &mut area,
                        list,
                        "Não foi possível carregar as propostas",
                        &format!("{e} Use Atualizar para tentar de novo."),
                    );
                    None
                }
                (_, None) => {
                    empty_in(
                        &mut area,
                        list,
                        "Carregando propostas…",
                        "Até 200 por estado.",
                    );
                    None
                }
            };
            if let Some(id) = picked {
                self.identifier = id;
                self.open = true;
            } else if entered && !self.identifier.trim().is_empty() {
                // Tela estreita: Enter no campo abre o detalhe do ID.
                self.open = true;
            }
            // O ID digitado vale ao sair do campo, já neste frame.
            self.select(shared, s);
            if wide {
                let detail = divide(ui, rect, column);
                ask = ask.or(self.proposal(ui, s, detail, false, now));
            }
        });
        let id = self.identifier.trim().to_owned();
        match ask {
            Some(0) => self.begin(
                shared,
                Work::Read(Read::Proposal {
                    scope: s.clone(),
                    id,
                }),
            ),
            Some(1) => self.preview(
                shared,
                s,
                Intent::Reject {
                    id,
                    reason: self.reason.clone(),
                },
            ),
            Some(2) => self.preview(shared, s, Intent::Approve { id }),
            _ => {}
        }
    }
    /// Pede o detalhe da proposta do campo de ID quando ela muda: um clique
    /// na linha ou o ID digitado, ao sair do campo (Enter, Tab ou um clique
    /// fora). Uma GET por seleção; a resposta de uma seleção anterior é
    /// ignorada (`Fetch`).
    fn select(&mut self, shared: &Shared, s: &Scope) {
        let key = (s.clone(), self.identifier.trim().to_owned());
        if !self.editing && !key.1.is_empty() && self.detail.key.as_ref() != Some(&key) {
            self.detail.request(shared, key);
        }
    }
    /// O pé da coluna da lista `column`, fixo embaixo dela: "ID da proposta",
    /// o campo e "Ver detalhes" (`ask` = 0). Devolve a área do pé e se o ID
    /// foi confirmado com Enter.
    fn id_foot(&mut self, ui: &mut Ui, column: Rect, ask: &mut Option<u8>) -> (Rect, bool) {
        let p = theme::palette(ui.visuals());
        let mut foot = ui.new_child(
            UiBuilder::new()
                .max_rect(column.shrink2(vec2(space::L, space::M)))
                .layout(Layout::bottom_up(Align::Min)),
        );
        foot.spacing_mut().item_spacing.y = space::XS;
        let mut entered = false;
        foot.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S;
            let width = (ui.available_width() - 110.).clamp(120., FIELD);
            let field = widgets::mono_field_with(ui, &mut self.identifier, width, "ID exato", 128);
            self.editing = field.has_focus();
            entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let id = !self.identifier.trim().is_empty();
            if ui
                .add_enabled(id, Button::secondary("Ver detalhes"))
                .clicked()
            {
                *ask = Some(0);
            }
        });
        widgets::caption(&mut foot, "ID da proposta");
        let top = foot.min_rect().top() - space::M;
        ui.painter()
            .hline(column.x_range(), top + 0.5, Stroke::new(1.0_f32, p.line));
        (
            Rect::from_min_max(pos2(column.left(), top), column.max),
            entered,
        )
    }
    /// O detalhe da proposta do campo de ID em `rect` (a GET da seleção): no
    /// topo, fixas, a linha de estado (altura reservada) e a do título, com
    /// Rejeitar… e Aprovar… à direita; o corpo, que rola; e o motivo da
    /// rejeição, fixo embaixo. Nada se move quando o detalhe chega. `back` =
    /// tela estreita, com "← Voltar" acima. Devolve a ação pedida (1 =
    /// Rejeitar, 2 = Aprovar).
    fn proposal(
        &mut self,
        ui: &mut Ui,
        s: &Scope,
        rect: Rect,
        back: bool,
        now: DateTime<Utc>,
    ) -> Option<u8> {
        let p = theme::palette(ui.visuals());
        widgets::pane(ui, rect, "Proposta selecionada");
        let inner = rect.shrink2(if back { vec2(0., 8.) } else { vec2(28., 22.) });
        // O motivo antes do resto: guarda o id (e o foco) quando o detalhe
        // muda.
        let mut foot = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::bottom_up(Align::Min)),
        );
        foot.spacing_mut().item_spacing.y = space::XS;
        let width = foot.available_width().min(FIELD);
        widgets::text_field(
            &mut foot,
            &mut self.reason,
            width,
            "Fica registrado com a rejeição",
            false,
            500,
        );
        widgets::caption(&mut foot, "Motivo da rejeição");
        let bottom = foot.min_rect().top() - 14.;
        let mut column = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(inner.min, pos2(inner.right(), bottom)))
                .layout(Layout::top_down(Align::Min)),
        );
        column.spacing_mut().item_spacing.y = 14.;
        if back && column.add(Button::secondary("← Voltar").small()).clicked() {
            self.open = false;
        }
        let key = (s.clone(), self.identifier.trim().to_owned());
        let loaded = (self.detail.key.as_ref() == Some(&key)).then_some(&self.detail.value);
        let value = match loaded {
            Some(Some(Ok(v))) => Some(v),
            _ => None,
        };
        // A linha de estado, com a altura de uma linha mesmo vazia.
        let full = column.max_rect();
        let line = column
            .painter()
            .layout_no_wrap(" ".into(), Type::Caption.font(), p.text2)
            .size()
            .y;
        column.allocate_ui_with_layout(
            vec2(full.width(), line),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_height(line);
                if let Some(meta) = value.and_then(|v| proposal_meta(v, now)) {
                    ui.add(Label::new(RichText::new(meta).size(12.).color(p.text2)).truncate());
                }
            },
        );
        // A linha do título: as ações à direita, fixas; o título à esquerda,
        // que quebra linha sem empurrá-las.
        let top = column.cursor().top();
        let mut actions = column.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(full.left(), top),
                    pos2(full.right(), top + 28.),
                ))
                .layout(Layout::right_to_left(Align::Center)),
        );
        actions.spacing_mut().item_spacing.x = space::S;
        let id = !key.1.is_empty();
        // O estado conhecido: o do detalhe ou, enquanto ele não chega, o da
        // linha da lista. Fora de pendente, os dois botões desabilitam no
        // lugar; desconhecido, ficam habilitados e a prévia decide.
        let state = value
            .and_then(|v| v["summary"]["status"].as_str())
            .or_else(|| {
                self.list
                    .loaded(&(s.clone(), self.status.clone()))?
                    .iter()
                    .find(|row| row["id"] == key.1)?["status"]
                    .as_str()
            });
        let pending = state.is_none_or(|state| state == "pending");
        let hint = |response: egui::Response| {
            if pending {
                response
            } else {
                response.on_disabled_hover_text(NOT_PENDING)
            }
        };
        let mut ask = None;
        if hint(actions.add_enabled(id && pending, Button::primary("Aprovar…"))).clicked() {
            ask = Some(2);
        }
        if hint(actions.add_enabled(id && pending, Button::destructive("Rejeitar…"))).clicked() {
            ask = Some(1);
        }
        let mut title = column.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(full.left(), top),
                    pos2(actions.min_rect().left() - space::M, full.bottom()),
                ))
                .layout(Layout::top_down(Align::Min)),
        );
        if let Some(text) = value
            .and_then(|v| v["summary"]["title"].as_str())
            .filter(|t| !t.trim().is_empty())
        {
            title.add(Label::new(RichText::new(text).font(Type::Detail.font())).wrap());
        }
        column.allocate_space(vec2(full.width(), title.min_rect().height().max(28.)));
        widgets::scroll_bar(&mut column);
        egui::ScrollArea::vertical()
            .id_salt(("detail", "Proposta selecionada"))
            .auto_shrink(false)
            .show(&mut column, |ui| {
                ui.set_max_width(ui.available_width() - DETAIL_GAP);
                ui.spacing_mut().item_spacing.y = 14.;
                match loaded {
                    _ if key.1.is_empty() => note(
                        ui,
                        "Nenhuma proposta selecionada",
                        "Escolha uma na lista ou informe o ID de uma proposta fora dela.",
                    ),
                    None => note(
                        ui,
                        "Proposta não carregada",
                        "Conclua o ID e pressione Enter para buscá-la no servidor.",
                    ),
                    Some(None) => note(
                        ui,
                        "Carregando proposta…",
                        "Os detalhes aparecem aqui quando o servidor responder.",
                    ),
                    Some(Some(Err(e))) => {
                        widgets::strip(
                            ui,
                            Tone::Bad,
                            "Não foi possível carregar a proposta.",
                            e,
                            None,
                        );
                    }
                    Some(Some(Ok(v))) => proposal_body(ui, v),
                }
            });
        ask
    }
    fn handoffs(
        &mut self,
        ui: &mut Ui,
        shared: &Shared,
        s: &Scope,
        api: &ApiSnapshot,
        now: DateTime<Utc>,
    ) {
        let shown = open_handoffs(api, s);
        widgets::split(ui, &[HANDOFF_NOTE.into()], |ui| {
            let rect = ui.max_rect();
            let wide = rect.width() >= SIDE_BY_SIDE;
            let current = shown
                .iter()
                .copied()
                .find(|h| h.id == self.identifier.trim());
            if !wide && self.open {
                self.handoff(ui, shared, s, rect, current, true, now);
                return;
            }
            let list = list_rect(rect, wide);
            // O landmark da lista, na mesma área em todo estado (linhas,
            // vazia ou aguardando a coleta).
            widgets::pane(ui, list, "Lista de handoffs");
            let picked = if shown.is_empty() {
                if api.at.is_some() {
                    empty_in(
                        ui,
                        list,
                        "Nenhum handoff aberto",
                        "Nenhum handoff aberto neste projeto na última coleta.",
                    );
                } else {
                    empty_in(
                        ui,
                        list,
                        "Aguardando a coleta",
                        "Os handoffs abertos vêm da coleta do monitor.",
                    );
                }
                None
            } else {
                rows_view(ui, list, "handoffs", shown.len(), !wide, |ui, rect, i| {
                    let h = shown[i];
                    let (title, preview) = headline(h);
                    let agent = if h.agent.is_empty() { "—" } else { &h.agent };
                    let second = if preview.is_empty() {
                        agent.to_owned()
                    } else {
                        format!("{agent} · {preview}")
                    };
                    let when = age(&h.at, now).map_or_else(|| "—".to_owned(), |a| a.short());
                    let picked = h.id == self.identifier.trim();
                    row_view(ui, rect, i, (&title, &second, &when), false, picked)
                        .clicked()
                        .then(|| h.id.clone())
                })
            };
            let foot = !wide && narrow_foot(ui, list);
            if let Some(id) = picked {
                self.identifier = id;
                self.open = true;
            } else if foot {
                self.open = true;
            }
            if wide {
                let detail = divide(ui, rect, list);
                self.handoff(ui, shared, s, detail, current, false, now);
            }
        });
    }
    /// O detalhe do handoff `h` (o do ID, se está na lista) em `rect`, com o
    /// ID e "Cancelar handoff…". `back` = tela estreita.
    #[allow(clippy::too_many_arguments)]
    fn handoff(
        &mut self,
        ui: &mut Ui,
        shared: &Shared,
        s: &Scope,
        rect: Rect,
        h: Option<&Handoff>,
        back: bool,
        now: DateTime<Utc>,
    ) {
        let mut cancel = false;
        detail(
            ui,
            rect,
            "Handoff selecionado",
            back,
            &mut self.open,
            |ui| {
                // Numa `Ui` própria: o campo de ID abaixo guarda o id (e o
                // foco) quando o que está acima dele muda.
                ui.scope(|ui| {
                    let p = theme::palette(ui.visuals());
                    match h {
                        Some(h) => {
                            let agent = if h.agent.is_empty() { "—" } else { &h.agent };
                            let when = dated(&h.at, now);
                            widgets::caption(ui, &format!("Aberto · {agent} · {when}"));
                            ui.add(
                                Label::new(RichText::new(headline(h).0).font(Type::Detail.font()))
                                    .wrap(),
                            );
                            let parsed = (!h.redacted)
                                .then_some(h.summary.as_deref())
                                .flatten()
                                .map(handoff_text::parse);
                            match &parsed {
                                _ if h.redacted => body(
                                    ui,
                                    "Resumo restrito pelas permissões do servidor.",
                                    p.text2,
                                ),
                                None => body(ui, "Sem resumo.", p.text2),
                                Some(parsed) => {
                                    let rest = handoff_text::outline(&parsed.body).rest;
                                    for paragraph in rest.split("\n\n") {
                                        let paragraph = paragraph.trim_matches('\n');
                                        if !paragraph.trim().is_empty() {
                                            body(ui, paragraph, p.text);
                                        }
                                    }
                                }
                            }
                            if let Some(steps) = h.next_steps.as_ref().filter(|s| !s.is_empty()) {
                                ui.label(
                                    RichText::new("Próximos passos")
                                        .size(12.)
                                        .family(theme::semibold())
                                        .color(p.text2),
                                );
                                for (i, step) in steps.iter().enumerate() {
                                    body(ui, &format!("{}. {step}", i + 1), p.text);
                                }
                            }
                        }
                        None => placeholder(
                            ui,
                            self.identifier.trim().is_empty(),
                            (
                                "Nenhum handoff selecionado",
                                "Escolha um na lista ou informe o ID de um handoff fora dela.",
                            ),
                            (
                                "Handoff fora da lista",
                                "Este ID não está entre os abertos deste projeto na última coleta.",
                            ),
                        ),
                    }
                });
                let id = !self.identifier.trim().is_empty();
                labeled(ui, "ID do handoff", |ui| {
                    let width = ui.available_width().min(FIELD);
                    widgets::mono_field_with(ui, &mut self.identifier, width, "ID exato", 128);
                });
                cancel = ui
                    .add_enabled(id, Button::destructive("Cancelar handoff…"))
                    .clicked();
            },
        );
        if cancel {
            let id = self.identifier.trim().into();
            self.preview(shared, s, Intent::CancelHandoff { id });
        }
    }
    fn maintenance(&mut self, ui: &mut Ui, shared: &Shared, s: &Scope) {
        widgets::caption(ui, MAINTENANCE_NOTE);
        let mut ask = None;
        cards(ui, |ui| {
            form(ui, "Tarefa de manutenção", |ui| {
                labeled_line(ui, "Tarefa", |ui| {
                    let options: Vec<(Task, String)> =
                        [Task::Lint, Task::Curator, Task::Sweep, Task::Embeddings]
                            .into_iter()
                            .map(|t| (t, t.label().to_owned()))
                            .collect();
                    let text = self.task.label();
                    picker(
                        ui,
                        "maintenance-task",
                        &mut self.task,
                        text,
                        FIELD,
                        &options,
                    );
                });
                if self.task == Task::Embeddings {
                    widgets::checkbox(
                        ui,
                        &mut self.reembed,
                        "Regenerar também embeddings já existentes",
                    );
                }
                let button = if self.task == Task::Sweep {
                    Button::destructive("Preparar prévia sem aplicar")
                } else {
                    Button::secondary("Preparar prévia sem aplicar")
                };
                if ui.add(button.raised()).clicked() {
                    ask = Some(Intent::Maintain {
                        task: self.task,
                        reembed: self.reembed,
                    });
                }
            });
            form(ui, "Reconsolidação de sessão", |ui| {
                field_line(ui, "ID exato da sessão", &mut self.session, 128);
                if ui
                    .add(Button::secondary("Preparar prévia de reconsolidação").raised())
                    .clicked()
                {
                    ask = Some(Intent::Consolidate {
                        session: self.session.trim().into(),
                    });
                }
            });
            form(ui, "Feedback de conhecimento", |ui| {
                field_line(ui, "Caminho da página", &mut self.path, 4096);
                labeled_line(ui, "Sinal", |ui| {
                    let options: Vec<(String, String)> = [
                        ("helpful", "Útil"),
                        ("not_helpful", "Não ajudou"),
                        ("stale", "Desatualizada"),
                        ("wrong", "Incorreta"),
                    ]
                    .into_iter()
                    .map(|(v, l)| (v.to_owned(), l.to_owned()))
                    .collect();
                    let text = options
                        .iter()
                        .find(|(v, _)| *v == self.signal)
                        .map_or(self.signal.clone(), |(_, l)| l.clone());
                    picker(
                        ui,
                        "feedback-signal",
                        &mut self.signal,
                        &text,
                        FIELD,
                        &options,
                    );
                });
                field_line(ui, "Motivo", &mut self.reason, 500);
                if ui
                    .add(Button::secondary("Revisar feedback").raised())
                    .clicked()
                {
                    ask = Some(Intent::Feedback {
                        path: self.path.trim().into(),
                        signal: self.signal.clone(),
                        reason: self.reason.clone(),
                    });
                }
            });
        });
        if let Some(intent) = ask {
            self.preview(shared, s, intent);
        }
    }
    fn backups(&mut self, ui: &mut Ui, shared: &Shared, s: &Scope) {
        widgets::caption(ui, MAINTENANCE_NOTE);
        let mut ask = None;
        let mut checkpoints = false;
        cards(ui, |ui| {
            form(ui, "Backup e exportação", |ui| {
                widgets::segmented(
                    ui,
                    &mut self.okf,
                    &[
                        Segment {
                            value: false,
                            label: "Backup global",
                            count: None,
                        },
                        Segment {
                            value: true,
                            label: "Exportar projeto (OKF)",
                            count: None,
                        },
                    ],
                );
                field_line(
                    ui,
                    "Arquivo de destino (caminho absoluto)",
                    &mut self.destination,
                    4096,
                );
                if ui
                    .add(Button::secondary("Revisar download privado").raised())
                    .clicked()
                {
                    ask = Some(Intent::Download {
                        destination: self.destination.trim().into(),
                        okf: self.okf,
                    });
                }
            });
            form(ui, "Checkpoints da wiki", |ui| {
                if ui
                    .add(Button::secondary("Listar últimos 100 checkpoints").raised())
                    .clicked()
                {
                    checkpoints = true;
                }
                field_line(ui, "Descrição do checkpoint", &mut self.message, 500);
                if ui
                    .add(Button::secondary("Revisar criação de checkpoint").raised())
                    .clicked()
                {
                    ask = Some(Intent::Checkpoint {
                        message: self.message.clone(),
                    });
                }
            });
            form(ui, "Restaurar uma página", |ui| {
                field_line(ui, "Caminho da página", &mut self.path, 4096);
                widgets::checkbox(
                    ui,
                    &mut self.git,
                    "Usar checkpoint Git (desmarcado: ID da versão da página)",
                );
                field_line(
                    ui,
                    "ID completo da versão ou checkpoint",
                    &mut self.version,
                    128,
                );
                if ui
                    .add(Button::secondary("Preparar restauração").raised())
                    .clicked()
                {
                    ask = Some(Intent::Restore {
                        path: self.path.trim().into(),
                        version: self.version.trim().into(),
                        git: self.git,
                    });
                }
            });
        });
        if checkpoints {
            self.begin(shared, Work::Read(Read::Checkpoints));
        }
        if let Some(intent) = ask {
            self.preview(shared, s, intent);
        }
    }
}

/// Os handoffs abertos de `scope` na coleta, na ordem dela.
fn open_handoffs<'a>(api: &'a ApiSnapshot, scope: &Scope) -> Vec<&'a Handoff> {
    let label = scope.label();
    api.handoffs.iter().filter(|h| h.scope == label).collect()
}

/// (valor, plural, singular) do estado; um estado desconhecido fica como veio.
fn status_label(status: &str) -> (&str, &str, &str) {
    STATUSES
        .iter()
        .copied()
        .find(|(value, ..)| *value == status)
        .unwrap_or((status, status, status))
}

/// A idade da proposta: `staged_at` em microssegundos (o formato do
/// servidor) ou uma data RFC 3339.
fn proposal_age(row: &Value, now: DateTime<Utc>) -> Option<crate::sessions_ui::Age> {
    let at = match &row["staged_at"] {
        Value::Number(n) => DateTime::from_timestamp_micros(n.as_i64()?)?.to_rfc3339(),
        Value::String(s) => s.clone(),
        _ => return None,
    };
    age(&at, now)
}

/// O corpo do detalhe da proposta `v`, a resposta de `GET
/// /admin/pending-writes/{id}` (`summary` com `status`, `title` e
/// `staged_at`, `rationale`, `evidence_json` = [{page, quote}] e `events`):
/// a justificativa e as evidências, em text2. A linha de estado
/// (`proposal_meta`) e o título ficam no topo do detalhe. Um campo ausente
/// some com a sua linha.
fn proposal_body(ui: &mut Ui, v: &Value) {
    fn text(value: &Value) -> Option<&str> {
        value.as_str().filter(|t| !t.trim().is_empty())
    }
    let p = theme::palette(ui.visuals());
    if let Some(rationale) = text(&v["rationale"]) {
        body(ui, rationale, p.text2);
    }
    let evidence: Vec<String> = v["evidence_json"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| match (text(&e["quote"]), text(&e["page"])) {
            (Some(quote), Some(page)) => Some(format!("“{quote}” — {page}")),
            (Some(quote), None) => Some(format!("“{quote}”")),
            (None, Some(page)) => Some(page.to_owned()),
            (None, None) => None,
        })
        .collect();
    if !evidence.is_empty() {
        ui.label(
            RichText::new("Evidências")
                .size(12.)
                .family(theme::semibold())
                .color(p.text2),
        );
        for line in &evidence {
            body(ui, line, p.text2);
        }
    }
}
/// "Proposta <estado> · criada por <agente> há N", sem as partes ausentes:
/// o estado do `summary`, o agente do evento `staged` (a criação) e a idade
/// de `staged_at`. `None` sem nenhuma das três.
fn proposal_meta(v: &Value, now: DateTime<Utc>) -> Option<String> {
    let status = v["summary"]["status"].as_str().map(|s| status_label(s).2);
    let agent = v["events"]
        .as_array()
        .and_then(|events| events.iter().find(|e| e["event"] == "staged"))
        .and_then(|e| e["actor_json"]["agent"].as_str())
        .filter(|a| !a.trim().is_empty());
    let when = proposal_age(&v["summary"], now)
        .map(|a| a.long().unwrap_or_else(|| format!("em {}", a.short())));
    let created = match (agent, when) {
        (Some(agent), Some(when)) => Some(format!("criada por {agent} {when}")),
        (Some(agent), None) => Some(format!("criada por {agent}")),
        (None, Some(when)) => Some(format!("criada {when}")),
        (None, None) => None,
    };
    match (status, created) {
        (None, None) => None,
        (Some(status), None) => Some(format!("Proposta {status}")),
        (None, Some(created)) => Some(format!("Proposta · {created}")),
        (Some(status), Some(created)) => Some(format!("Proposta {status} · {created}")),
    }
}

/// As ações da aba à direita da linha de ferramentas, montadas da direita
/// para a esquerda: Propostas = Atualizar e Estado; Handoffs = expirar
/// (com a idade) e o histórico; as de cada aba travadas com `locked`.
fn actions(ui: &mut Ui, tab: u8, status: &mut String, days: &mut u32, locked: bool) -> Option<Ask> {
    match tab {
        0 => {
            let mut ask = None;
            ui.add_enabled_ui(!locked, |ui| {
                if ui.add(Button::secondary("Atualizar")).clicked() {
                    ask = Some(Ask::Refresh);
                }
                let options: Vec<(String, String)> = STATUSES
                    .iter()
                    .map(|(value, plural, _)| ((*value).to_owned(), format!("Estado: {plural}")))
                    .collect();
                let text = format!("Estado: {}", status_label(status).1);
                picker(ui, "proposal-state", status, &text, PROJECT_WIDTH, &options);
            });
            ask
        }
        1 => {
            let mut ask = None;
            ui.add_enabled_ui(!locked, |ui| {
                if ui.add(Button::destructive("Expirar antigos…")).clicked() {
                    ask = Some(Ask::Expire);
                }
                ui.scope(|ui| {
                    // Como os campos de texto: 26 px, raio 5, fundo field,
                    // borda line (acc ao digitar) e texto 12.
                    let p = theme::palette(ui.visuals());
                    let visuals = ui.visuals_mut();
                    visuals.text_edit_bg_color = Some(p.field);
                    let widgets = &mut visuals.widgets;
                    for state in [
                        &mut widgets.inactive,
                        &mut widgets.hovered,
                        &mut widgets.active,
                    ] {
                        state.bg_fill = p.field;
                        state.weak_bg_fill = p.field;
                        state.bg_stroke = Stroke::new(1.0_f32, p.line);
                        state.corner_radius = egui::CornerRadius::same(5);
                        state.expansion = 0.;
                    }
                    widgets.active.bg_stroke = Stroke::new(1.0_f32, p.acc);
                    ui.spacing_mut().button_padding = vec2(8., 4.);
                    ui.spacing_mut().interact_size.y = 26.;
                    ui.style_mut().drag_value_text_style = egui::TextStyle::Small;
                    ui.add(egui::DragValue::new(days).range(0..=36500).suffix(" dias"))
                        .on_hover_text("Idade mínima dos handoffs a expirar. Zero dias abrange todos os abertos de sua propriedade ou compartilhados neste projeto.");
                });
                widgets::caption(ui, "Idade mínima");
                if ui.add(Button::secondary("Carregar histórico")).clicked() {
                    ask = Some(Ask::History);
                }
            });
            ask
        }
        _ => None,
    }
}
/// A largura de `actions` (textos, margens e espaços), para decidir se cabem.
fn actions_width(ui: &Ui, tab: u8, status: &str) -> f32 {
    let width = |text: &str, size: f32| {
        ui.painter()
            .layout_no_wrap(
                text.to_owned(),
                FontId::proportional(size),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .x
    };
    match tab {
        0 => {
            width("Atualizar", 13.)
                + 24.
                + width(&format!("Estado: {}", status_label(status).1), 12.)
                + 2. * 10.
                + 6.
                + 10.
                + space::S
        }
        1 => {
            width("Expirar antigos…", 13.)
                + width("Carregar histórico", 13.)
                + 2. * 24.
                + width("Idade mínima", 12.)
                + 90.
                + 3. * space::S
        }
        _ => 0.,
    }
}

/// Seletor no estilo do botão secundário de 26 px (Review.dc.html): fundo
/// surf, borda line, raio 6, texto 12 e a seta fina de 10 px; corta além de
/// `width` (o texto inteiro vai ao tooltip). AccessKit: `ComboBox` com o
/// texto como valor.
fn picker<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: &str,
    current: &mut T,
    text: &str,
    width: f32,
    options: &[(T, String)],
) {
    let p = theme::palette(ui.visuals());
    ui.scope(|ui| {
        let spacing = ui.spacing_mut();
        spacing.interact_size.y = 26.;
        spacing.button_padding = vec2(10., 4.);
        spacing.icon_width = 10.;
        spacing.icon_spacing = 6.;
        ui.style_mut()
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(12.));
        let widgets = &mut ui.visuals_mut().widgets;
        for (state, fill) in [
            (&mut widgets.inactive, p.surf),
            (&mut widgets.hovered, p.surf2),
            (&mut widgets.active, p.surf2),
            (&mut widgets.open, p.surf2),
        ] {
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0_f32, p.line);
            state.corner_radius = 6.into();
            state.expansion = 0.;
        }
        let natural = picker_natural(ui, text);
        ui.allocate_ui_with_layout(
            vec2(width.min(natural).max(80.), 26.),
            Layout::left_to_right(Align::Center),
            |ui| {
                let response = egui::ComboBox::from_id_salt(id)
                    .selected_text(text)
                    .width(0.)
                    .truncate()
                    .icon(|ui, rect, _, _| {
                        let p = theme::palette(ui.visuals());
                        let icon = Rect::from_center_size(rect.center(), egui::Vec2::splat(10.));
                        widgets::chevron(ui.painter(), icon, true, p.text);
                    })
                    .show_ui(ui, |ui| {
                        for (value, label) in options {
                            ui.selectable_value(current, value.clone(), label);
                        }
                    })
                    .response;
                if response.rect.width() + 0.5 < natural {
                    response.on_hover_text(text);
                }
            },
        );
    });
}

/// A largura de `picker` sem corte: o texto (12), a seta (10, a 6 dele) e
/// as margens (10 de cada lado).
fn picker_natural(ui: &Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            FontId::proportional(12.),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
        + 6.
        + 10.
        + 2. * 10.
}
/// A coluna da lista: 35 % entre `LIST_MIN` e `LIST_MAX` lado a lado; a
/// área toda na tela estreita.
fn list_rect(rect: Rect, wide: bool) -> Rect {
    if wide {
        let width = (rect.width() * 0.35).clamp(LIST_MIN, LIST_MAX);
        Rect::from_min_size(rect.min, vec2(width, rect.height()))
    } else {
        rect
    }
}
/// A divisória à direita da lista; devolve a área do detalhe.
fn divide(ui: &mut Ui, rect: Rect, list: Rect) -> Rect {
    let p = theme::palette(ui.visuals());
    ui.painter().vline(
        list.right() + 0.5,
        rect.y_range(),
        Stroke::new(1.0_f32, p.line),
    );
    Rect::from_min_max(pos2(list.right() + 1., rect.top()), rect.max)
}
/// Mensagem no centro de `rect`.
fn empty_in(ui: &mut Ui, rect: Rect, title: &str, subtitle: &str) {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink(space::L))
            .layout(Layout::top_down(Align::Center)),
    );
    widgets::empty(&mut child, title, subtitle);
}
/// O pé da lista na tela estreita, com "Informar ID…" (o detalhe, para um
/// ID fora da lista). Devolve o clique.
fn narrow_foot(ui: &mut Ui, list: Rect) -> bool {
    let foot = Rect::from_min_max(pos2(list.left(), list.bottom() - LIST_FOOT), list.max);
    ui.new_child(
        UiBuilder::new()
            .max_rect(foot)
            .layout(Layout::right_to_left(Align::Center)),
    )
    .add(Button::secondary("Informar ID…").small())
    .clicked()
}
/// A lista rolável de `n` linhas de `ROW_HEIGHT` em `rect` (menos o pé, na
/// tela estreita); só as linhas visíveis são montadas. `row` desenha a
/// linha `i` e devolve o ID clicado. O landmark (`widgets::pane`) é de quem
/// chama: existe também sem linhas.
fn rows_view(
    ui: &mut Ui,
    rect: Rect,
    id: &str,
    n: usize,
    foot: bool,
    mut row: impl FnMut(&mut Ui, Rect, usize) -> Option<String>,
) -> Option<String> {
    let bottom = if foot {
        rect.bottom() - LIST_FOOT
    } else {
        rect.bottom()
    };
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(rect.min, pos2(rect.right(), bottom)))
            .layout(Layout::top_down(Align::Min)),
    );
    widgets::scroll_bar(&mut child);
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink(false)
        .show_viewport(&mut child, |ui, viewport| {
            ui.set_height(ROW_HEIGHT * n as f32);
            let content = ui.max_rect();
            let first = (viewport.min.y / ROW_HEIGHT).floor().max(0.) as usize;
            let last = ((viewport.max.y / ROW_HEIGHT).ceil() as usize).min(n);
            for i in first..last {
                let at = Rect::from_min_size(
                    pos2(content.left(), content.top() + i as f32 * ROW_HEIGHT),
                    vec2(content.width(), ROW_HEIGHT),
                );
                if let Some(id) = row(ui, at, i) {
                    clicked = Some(id);
                }
            }
        });
    clicked
}
/// Uma linha da lista: o título (13) com a idade à direita (11, text2) e a
/// segunda linha em text2 (mono 11 em `mono`, o alvo da proposta). A
/// selecionada tem o fundo sel e a faixa acc de 3 px à esquerda.
/// AccessKit: `Button` "título · segunda · idade", marcado = selecionado.
fn row_view(
    ui: &mut Ui,
    rect: Rect,
    index: usize,
    (title, second, when): (&str, &str, &str),
    mono: bool,
    picked: bool,
) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let response = ui.interact(rect, ui.id().with(("row", index)), Sense::click());
    let spoken = format!("{title} · {second} · {when}");
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            picked,
            &spoken,
        )
    });
    let painter = ui.painter();
    if picked {
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
    let (left, right) = (rect.left() + 16., rect.right() - 16.);
    let age = painter.layout_no_wrap(when.to_owned(), FontId::proportional(11.), p.text2);
    let title = widgets::one_line(
        ui,
        title,
        Type::Body.font(),
        p.text,
        right - left - age.size().x - 8.,
    );
    let font = if mono {
        Type::Mono.font()
    } else {
        Type::Caption.font()
    };
    let second = widgets::one_line(ui, second, font, p.text2, right - left);
    let elided = title.elided || second.elided;
    let top = rect.center().y - (title.size().y + 3. + second.size().y) / 2.;
    let title_height = title.size().y;
    painter.galley(
        pos2(
            right - age.size().x,
            top + (title_height - age.size().y) / 2.,
        ),
        age,
        p.text2,
    );
    painter.galley(pos2(left, top), title, p.text);
    painter.galley(pos2(left, top + title_height + 3.), second, p.text2);
    if elided {
        response.on_hover_text(&spoken)
    } else {
        response
    }
}
/// O detalhe em `rect`: margem 28 × 22 lado a lado; na tela estreita, sem
/// margem lateral e com "← Voltar" (que fecha `open`). O conteúdo rola.
fn detail(
    ui: &mut Ui,
    rect: Rect,
    pane: &str,
    back: bool,
    open: &mut bool,
    content: impl FnOnce(&mut Ui),
) {
    widgets::pane(ui, rect, pane);
    let inner = rect.shrink2(if back { vec2(0., 8.) } else { vec2(28., 22.) });
    let mut column = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    if back && column.add(Button::secondary("← Voltar").small()).clicked() {
        *open = false;
    }
    widgets::scroll_bar(&mut column);
    egui::ScrollArea::vertical()
        .id_salt(("detail", pane))
        .auto_shrink(false)
        .show(&mut column, |ui| {
            ui.set_max_width(ui.available_width() - DETAIL_GAP);
            ui.spacing_mut().item_spacing.y = 14.;
            content(ui);
        });
}
/// O detalhe sem item da lista: nada escolhido (`none`) ou um ID fora da
/// lista (`outside`): (título, legenda).
fn placeholder(ui: &mut Ui, blank: bool, none: (&str, &str), outside: (&str, &str)) {
    let (title, subtitle) = if blank { none } else { outside };
    note(ui, title, subtitle);
}
/// Um aviso no lugar do detalhe: o título (Seção) e a legenda.
fn note(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        ui.label(RichText::new(title).font(Type::Section.font()));
        widgets::caption(ui, subtitle);
    });
}
/// Um campo com o rótulo (12, text2) em cima.
fn labeled(ui: &mut Ui, label: &str, field: impl FnOnce(&mut Ui)) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        widgets::caption(ui, label);
        field(ui);
    });
}
/// Parágrafo do corpo de um handoff: 13 px, entrelinha 20.
fn body(ui: &mut Ui, text: &str, color: egui::Color32) {
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
/// Os cartões de uma aba de Manutenção e backup, um abaixo do outro.
fn cards(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = space::M;
        add(ui);
    });
}
/// Um cartão de formulário: o título (Seção) e as linhas, até `CARD`.
fn form(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    widgets::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width().min(CARD));
        ui.spacing_mut().item_spacing.y = space::M;
        ui.label(RichText::new(title).font(Type::Section.font()));
        add(ui);
    });
}
/// Uma linha rótulo/campo: o rótulo na coluna `LABEL`, a 8 do controle (o
/// longo quebra a linha), e o controle no resto; sem largura para os dois,
/// o controle desce.
fn labeled_line(ui: &mut Ui, label: &str, field: impl FnOnce(&mut Ui)) {
    if ui.available_width() < LABEL + 160. {
        labeled(ui, label, field);
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.;
        ui.allocate_ui_with_layout(
            vec2(LABEL, 26.),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_size(vec2(LABEL, 26.));
                ui.allocate_ui(vec2(LABEL - space::S, 26.), |ui| {
                    ui.add(Label::new(RichText::new(label).font(Type::Body.font())).wrap());
                });
            },
        );
        field(ui);
    });
}
/// Uma linha rótulo/campo com um `text_field` de 26 px.
fn field_line(ui: &mut Ui, label: &str, value: &mut String, limit: usize) {
    labeled_line(ui, label, |ui| {
        let width = ui.available_width().min(FIELD);
        widgets::text_field(ui, value, width, "", false, limit);
    });
}

/// A prévia de Aprovar ou Rejeitar, quando a resposta tem o formato do
/// servidor: `proposal` = `GET /admin/pending-writes/{id}` (`summary` com
/// `target_path` e `operation`, e `body_markdown`) e `comparison.diff` =
/// "--- before/{alvo}\n+++ after/{alvo}\n@@\n{atual}\n--- proposed ---\n{proposto}\n"
/// (a página atual inteira e a proposta inteira, não um diff de linhas).
struct Proposed {
    target: String,
    /// (Efeito, Reversível); `None` com uma operação desconhecida. Rejeitar
    /// não tem Reversível.
    consequences: Option<(&'static str, Option<&'static str>)>,
    lines: Vec<(Mark, String)>,
}
#[cfg(test)]
thread_local! {
    /// Quantas vezes `proposed` calculou a prévia neste thread.
    static PROPOSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn proposed(intent: &Intent, result: &Value) -> Option<Proposed> {
    #[cfg(test)]
    PROPOSED.with(|n| n.set(n.get() + 1));
    if !matches!(intent, Intent::Approve { .. } | Intent::Reject { .. }) {
        return None;
    }
    let proposal = &result["proposal"];
    let target = proposal["summary"]["target_path"].as_str()?;
    let after = proposal["body_markdown"].as_str()?;
    let before = result["comparison"]["diff"]
        .as_str()?
        .strip_prefix(&format!("--- before/{target}\n+++ after/{target}\n@@\n"))?
        .strip_suffix(&format!("\n--- proposed ---\n{after}\n"))?;
    let operation = proposal["summary"]["operation"]
        .as_str()
        .unwrap_or_default();
    let consequences = match (intent, operation) {
        (Intent::Reject { .. }, _) => Some(("Recusa a proposta; a página não muda", None)),
        (_, "create") => Some((
            "Cria a página",
            Some("Não há versão anterior para restaurar"),
        )),
        (_, "update") => Some((
            "Grava uma nova versão da página; a atual fica no histórico",
            Some("Sim, restaurando a versão anterior em Manutenção e backup"),
        )),
        _ => None,
    };
    Some(Proposed {
        target: target.to_owned(),
        consequences,
        lines: changes(before, after),
    })
}

/// Uma linha da prévia em diff.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    Same,
    Removed,
    Added,
    /// Linhas iguais omitidas (o texto diz quantas).
    Skipped,
}
/// As linhas de `before` a `after`: o maior trecho comum (LCS) entre o
/// prefixo e o sufixo iguais, com `CONTEXT` linhas iguais em volta de cada
/// mudança e o resto resumido. Acima de `DIFF_CELLS` comparações, o meio
/// diferente sai inteiro como removido e acrescentado.
fn changes(before: &str, after: &str) -> Vec<(Mark, String)> {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (x, y) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let mut lines: Vec<(Mark, &str)> = a[..head].iter().map(|l| (Mark::Same, *l)).collect();
    let (n, m) = (x.len(), y.len());
    if n * m <= DIFF_CELLS {
        // common[i][j] = o maior trecho comum de x[i..] e y[j..].
        let mut common = vec![0u32; (n + 1) * (m + 1)];
        let at = |i: usize, j: usize| i * (m + 1) + j;
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                common[at(i, j)] = if x[i] == y[j] {
                    common[at(i + 1, j + 1)] + 1
                } else {
                    common[at(i + 1, j)].max(common[at(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && x[i] == y[j] {
                lines.push((Mark::Same, x[i]));
                i += 1;
                j += 1;
            } else if i < n && (j == m || common[at(i + 1, j)] >= common[at(i, j + 1)]) {
                lines.push((Mark::Removed, x[i]));
                i += 1;
            } else {
                lines.push((Mark::Added, y[j]));
                j += 1;
            }
        }
    } else {
        lines.extend(x.iter().map(|l| (Mark::Removed, *l)));
        lines.extend(y.iter().map(|l| (Mark::Added, *l)));
    }
    lines.extend(a[a.len() - tail..].iter().map(|l| (Mark::Same, *l)));
    let near = |i: usize| {
        lines[i.saturating_sub(CONTEXT)..(i + CONTEXT + 1).min(lines.len())]
            .iter()
            .any(|(mark, _)| *mark != Mark::Same)
    };
    let mut out = Vec::new();
    let mut skipped = 0;
    for (i, &(mark, line)) in lines.iter().enumerate() {
        if mark == Mark::Same && !near(i) {
            skipped += 1;
            continue;
        }
        if skipped > 0 {
            out.push((Mark::Skipped, skip(skipped)));
            skipped = 0;
        }
        out.push((mark, line.to_owned()));
    }
    if skipped > 0 {
        out.push((Mark::Skipped, skip(skipped)));
    }
    out
}
fn skip(n: usize) -> String {
    if n == 1 {
        "⋯ 1 linha igual".into()
    } else {
        format!("⋯ {} linhas iguais", number(n))
    }
}
/// A prévia em diff (Review.dc.html): mono, "−" em bad sobre bad_bg, "+" em
/// acc sobre add_bg, as iguais e o resumo em text2. AccessKit: um `Label`
/// por linha, com o sinal.
fn diff_view(ui: &mut Ui, lines: &[(Mark, String)]) {
    let p = theme::palette(ui.visuals());
    ui.spacing_mut().item_spacing.y = 0.;
    let width = ui.available_width();
    for (mark, line) in lines {
        let (sign, color, fill) = match mark {
            Mark::Removed => ("- ", p.bad, p.bad_bg),
            Mark::Added => ("+ ", p.acc, p.add_bg),
            Mark::Same => ("  ", p.text2, egui::Color32::TRANSPARENT),
            Mark::Skipped => ("", p.text2, egui::Color32::TRANSPARENT),
        };
        let text = format!("{sign}{line}");
        let galley = ui
            .painter()
            .layout(text.clone(), Type::Mono.font(), color, width - 8.);
        let (rect, response) =
            ui.allocate_exact_size(vec2(width, galley.size().y + 2.), Sense::hover());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &text));
        ui.painter().rect_filled(rect, 0., fill);
        ui.painter().galley(rect.min + vec2(4., 1.), galley, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        maintenance::PREVIEW_TTL,
        test_support::{Mock, Request, Response},
        widgets::tests::{
            click, click_sized, context, focused, node, nodes, rect_of, rects, run, run_sized,
            tab_and_enter, texts,
        },
    };
    use egui::accesskit::{Node, Role, Toggled};
    use serde_json::json;
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering::SeqCst},
            mpsc,
        },
        time::{Duration, Instant},
    };

    fn scope() -> Scope {
        Scope {
            workspace: "fixture".into(),
            project: "demo".into(),
        }
    }
    fn other() -> Scope {
        Scope {
            workspace: "fixture".into(),
            project: "other".into(),
        }
    }
    fn projects(scopes: &[Scope]) -> ApiSnapshot {
        ApiSnapshot {
            projects: scopes
                .iter()
                .map(|s| Project {
                    workspace_name: s.workspace.clone(),
                    project_name: s.project.clone(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    fn receipt() -> Receipt {
        Receipt {
            action: Action {
                scope: scope(),
                intent: Intent::Checkpoint {
                    message: "test".into(),
                },
            },
            generation: 0,
            at: std::time::Instant::now(),
        }
    }
    /// O recibo de uma prévia pedida há `secs` segundos.
    fn aged(secs: u64) -> Receipt {
        Receipt {
            at: Instant::now() - Duration::from_secs(secs),
            ..receipt()
        }
    }
    /// A proposta `i` da lista, criada há 25 min.
    fn proposal(i: usize) -> Value {
        let at = Utc::now() - chrono::Duration::minutes(25);
        json!({
            "id": format!("p-{i}"),
            "status": "pending",
            "operation": "update",
            "target_path": format!("decisions/p-{i}.md"),
            "kind": "decision",
            "title": format!("Proposta {i}"),
            "staged_at": at.timestamp_micros(),
        })
    }
    /// Servidor sintético: a GET da lista responde `list(projeto, estado)`;
    /// o resto, 404.
    fn server(list: impl Fn(&str, &str) -> Response + Send + 'static) -> Mock {
        Mock::new(
            move |r| match r.path.strip_prefix("/admin/pending-writes?") {
                Some(query) => {
                    let get = |key: &str| {
                        query
                            .split('&')
                            .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
                            .unwrap_or_default()
                            .to_owned()
                    };
                    list(&get("project"), &get("status"))
                }
                None => Response::json(404, json!({})),
            },
        )
    }
    /// Um painel num servidor sintético com a lista vazia.
    fn quiet() -> (Mock, Arc<Shared>, Management) {
        let mock = server(|_, _| Response::json(200, json!([])));
        let shared = Shared::unstarted(mock.config());
        let m = Management::new(shared.clone());
        (mock, shared, m)
    }
    /// As GETs da lista de propostas de `project`.
    fn listed(mock: &Mock, project: &str) -> usize {
        mock.requests()
            .iter()
            .filter(|r| {
                r.path.starts_with("/admin/pending-writes?")
                    && r.path.contains(&format!("project={project}&"))
            })
            .count()
    }
    /// O que não é a GET da lista.
    fn sent(mock: &Mock) -> Vec<Request> {
        mock.requests()
            .into_iter()
            .filter(|r| !r.path.starts_with("/admin/pending-writes?"))
            .collect()
    }
    fn page(
        m: &mut Management,
        ui: &mut Ui,
        shared: &Arc<Shared>,
        api: &ApiSnapshot,
        section: Section,
    ) -> Option<Section> {
        m.show(ui, shared, api, section, Utc::now())
    }
    /// Teto de segurança das esperas dos testes (e das porteiras dos
    /// servidores sintéticos): cada uma termina assim que a condição vale;
    /// o prazo só impede que um teste quebrado trave para sempre. Uma GET sem
    /// resposta falha no prazo do cliente (`client::TIMEOUT`); a folga cobre
    /// a volta do trabalhador, e passar dela é falha, com o que se esperava.
    const PATIENCE: Duration = crate::client::TIMEOUT.saturating_add(Duration::from_secs(3));
    /// Espera a resposta do trabalhador.
    fn settle(m: &mut Management, shared: &Shared) {
        let until = Instant::now() + PATIENCE;
        while m.busy {
            assert!(Instant::now() < until, "the worker never answered");
            std::thread::sleep(Duration::from_millis(2));
            m.poll(shared);
        }
    }
    /// Espera a resposta da lista.
    fn settle_list(m: &mut Management, shared: &Shared) {
        let until = Instant::now() + PATIENCE;
        while m.list.value.is_none() {
            assert!(Instant::now() < until, "the list never answered");
            std::thread::sleep(Duration::from_millis(2));
            m.poll(shared);
        }
    }
    /// Espera a resposta do detalhe.
    fn settle_detail(m: &mut Management, shared: &Shared) {
        let until = Instant::now() + PATIENCE;
        while m.detail.value.is_none() {
            assert!(Instant::now() < until, "the detail never answered");
            std::thread::sleep(Duration::from_millis(2));
            m.poll(shared);
        }
    }
    /// A resposta de `GET /admin/pending-writes/p-{i}` no formato do
    /// servidor (o `AutoImproveProposalDetail` do ai-memory): a linha `i` da
    /// lista, a justificativa, uma evidência e o histórico, com a criação
    /// (`staged`) por claude-code e a aprovação por outro agente.
    fn proposal_detail(i: usize) -> Value {
        let summary = proposal(i);
        json!({
            "summary": summary,
            "rationale": format!("Justificativa da proposta {i}."),
            "evidence_json": [{"page": "sessions/a.md", "quote": "expirou"}],
            "body_markdown": "# Corpo",
            "events": [
                {"id": 1, "proposal_id": format!("p-{i}"), "event": "staged",
                 "actor_json": {"agent": "claude-code"}, "author_id": null,
                 "detail_json": {}, "at": summary["staged_at"]},
                {"id": 2, "proposal_id": format!("p-{i}"), "event": "approved",
                 "actor_json": {"agent": "revisor"}, "author_id": null,
                 "detail_json": {}, "at": summary["staged_at"]},
            ],
        })
    }
    /// O N de `GET /admin/pending-writes/p-N?…`: o detalhe, não o diff.
    fn detail_of(path: &str) -> Option<usize> {
        path.strip_prefix("/admin/pending-writes/p-")?
            .split_once('?')?
            .0
            .parse()
            .ok()
    }
    /// Servidor sintético: a lista são `rows` e o detalhe de qualquer `p-N`
    /// é `proposal_detail(N)`; o resto, 404.
    fn catalog(rows: Vec<Value>) -> Mock {
        Mock::new(move |r| {
            if r.path.starts_with("/admin/pending-writes?") {
                Response::json(200, Value::Array(rows.clone()))
            } else if let Some(i) = detail_of(&r.path) {
                Response::json(200, proposal_detail(i))
            } else {
                Response::json(404, json!({}))
            }
        })
    }
    /// As GETs do detalhe da proposta `id`.
    fn read(mock: &Mock, id: &str) -> usize {
        mock.requests()
            .iter()
            .filter(|r| {
                r.method == "GET" && r.path.starts_with(&format!("/admin/pending-writes/{id}?"))
            })
            .count()
    }
    /// O item do segmentado (rótulo com ou sem o contador).
    fn segment(out: &egui::FullOutput, label: &str) -> Node {
        let found: Vec<Node> = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.role() == Role::Button
                    && n.label().is_some_and(|l| {
                        l == label || l.strip_prefix(label).is_some_and(|r| r.starts_with(' '))
                    })
            })
            .map(|(_, n)| n.clone())
            .collect();
        assert_eq!(found.len(), 1, "segment {label}");
        found[0].clone()
    }
    /// As linhas da lista (botões com " · "), de cima para baixo: (rótulo,
    /// selecionada).
    fn rows_of(out: &egui::FullOutput) -> Vec<(String, bool)> {
        let mut found: Vec<(f64, String, bool)> = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.role() == Role::Button && n.label().is_some_and(|l| l.contains(" · "))
            })
            .map(|(_, n)| {
                (
                    n.bounds().map_or(0., |b| b.y0),
                    n.label().unwrap().to_owned(),
                    n.toggled() == Some(Toggled::True),
                )
            })
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|(_, l, s)| (l, s)).collect()
    }
    /// Escolhe `option` no seletor que mostra `shown`: abre e clica.
    fn choose(
        ctx: &egui::Context,
        size: [f32; 2],
        add: &mut impl FnMut(&mut Ui),
        shown: &str,
        option: &str,
    ) {
        let out = run_sized(ctx, size, vec![], &mut *add);
        let combo = rect_of(&node(&out, Role::ComboBox, shown));
        click_sized(ctx, size, combo.center(), &mut *add);
        let out = run_sized(ctx, size, vec![], &mut *add);
        let item = rect_of(&node(&out, Role::Button, option));
        click_sized(ctx, size, item.center(), &mut *add);
    }

    #[test]
    fn every_tab_belongs_to_exactly_one_page() {
        // (aba, página): uma aba nova tem de entrar nesta tabela.
        for (tab, want) in [
            (0, Section::Review),
            (1, Section::Review),
            (2, Section::Maintenance),
            (3, Section::Maintenance),
        ] {
            let owners: Vec<Section> = Section::ALL
                .into_iter()
                .filter(|s| s.tabs().iter().any(|&(id, _)| id == tab))
                .collect();
            assert_eq!(owners, vec![want], "tab {tab}");
            assert_eq!(Section::of(tab), want, "tab {tab}");
        }
        let mut all: Vec<u8> = Section::ALL
            .into_iter()
            .flat_map(|s| s.tabs().map(|(id, _)| id))
            .collect();
        all.sort_unstable();
        assert_eq!(all, [0, 1, 2, 3], "no tab outside the table");
        for section in Section::ALL {
            for (id, _) in section.tabs() {
                assert_eq!(Section::of(id), section, "{section:?} tab {id}");
            }
        }
    }

    #[test]
    fn each_page_shows_only_its_tabs_and_opens_on_its_first_one() {
        for (tab, section, want) in [
            (0, Section::Review, 0),
            (1, Section::Review, 1),
            (2, Section::Review, 0),
            (3, Section::Review, 0),
            (0, Section::Maintenance, 2),
            (1, Section::Maintenance, 2),
            (2, Section::Maintenance, 2),
            (3, Section::Maintenance, 3),
        ] {
            let shared = Shared::unstarted(crate::model::Config::default());
            let mut m = Management::new(shared.clone());
            m.tab = tab;
            m.identifier = "draft".into();
            let ctx = context(egui::Theme::Dark);
            let out = run(&ctx, vec![], |ui| {
                assert_eq!(
                    page(&mut m, ui, &shared, &ApiSnapshot::default(), section),
                    None
                );
            });
            assert_eq!(m.tab, want, "tab {tab} on {section:?}");
            assert_eq!(
                m.identifier.is_empty(),
                tab != want,
                "a tab switch discards the fields, staying keeps them"
            );
            let (mine, theirs) = match section {
                Section::Review => (["Propostas", "Handoffs"], ["Manutenção", "Backup"]),
                Section::Maintenance => (["Manutenção", "Backup"], ["Propostas", "Handoffs"]),
            };
            for label in mine {
                node(&out, Role::Button, label);
            }
            for label in theirs {
                assert!(
                    nodes(&out, Role::Button, label).is_empty(),
                    "{label} on {section:?}"
                );
            }
        }
    }

    #[test]
    fn the_segmented_reports_the_selected_tab_to_accesskit() {
        let rows: Vec<Value> = (1..=3).map(proposal).collect();
        let mock = server(move |_, _| Response::json(200, Value::Array(rows.clone())));
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let mut api = projects(&[scope()]);
        api.at = Some(Utc::now());
        api.handoffs = ["h-1", "h-2"]
            .into_iter()
            .map(|id| Handoff {
                id: id.into(),
                scope: scope().label(),
                ..Default::default()
            })
            .collect();
        let ctx = context(egui::Theme::Light);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run(&ctx, vec![], &mut add);
        let state = |out: &egui::FullOutput, label| node(out, Role::Button, label).toggled();
        assert_eq!(state(&out, "Propostas 3"), Some(Toggled::True));
        assert_eq!(state(&out, "Handoffs 2"), Some(Toggled::False));
        click(
            &ctx,
            rect_of(&node(&out, Role::Button, "Handoffs 2")).center(),
            &mut add,
        );
        let out = run(&ctx, vec![], &mut add);
        drop(add);
        assert_eq!(m.tab, 1);
        assert_eq!(state(&out, "Propostas 3"), Some(Toggled::False));
        assert_eq!(state(&out, "Handoffs 2"), Some(Toggled::True));
        let out = run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Maintenance);
        });
        assert_eq!(state(&out, "Manutenção"), Some(Toggled::True));
        assert_eq!(state(&out, "Backup"), Some(Toggled::False));
    }

    #[test]
    fn the_caption_without_a_project_is_the_one_of_each_page() {
        let review = "Selecione o projeto para continuar.";
        let maintenance = "Selecione o projeto para continuar. Backup e checkpoints indicam explicitamente quando abrangem toda a instalação.";
        for section in [Section::Review, Section::Maintenance] {
            // Uma página nova tem de entrar nesta tabela.
            let (mine, theirs) = match section {
                Section::Review => (review, maintenance),
                Section::Maintenance => (maintenance, review),
            };
            let (_mock, shared, mut m) = quiet();
            let ctx = context(egui::Theme::Light);
            let api = ApiSnapshot::default();
            let out = run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, section);
            });
            node(&out, Role::Label, mine);
            assert!(
                nodes(&out, Role::Label, theirs).is_empty(),
                "{section:?}: not the caption of the other page"
            );
            // Com o projeto escolhido, a legenda sai.
            m.scope = Some(scope());
            let out = run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, section);
            });
            assert!(nodes(&out, Role::Label, mine).is_empty(), "{section:?}");
            segment(&out, section.tabs()[0].1);
        }
    }

    #[test]
    fn the_other_page_never_discards_what_holds_the_panel() {
        let result = "Resultado ainda na tela em Revisão · Propostas. Confira-o lá ou descarte-o para usar esta página.";
        let error = "Erro ainda na tela em Revisão · Propostas. Confira-o lá ou descarte-o para usar esta página.";
        // (estado que prende o painel em Revisão · Propostas, aviso, pode descartar?)
        for (case, warning, discard) in [
            (
                "busy",
                "Operação em andamento em Revisão · Propostas. Aguarde a resposta lá; esta página usa o mesmo painel.",
                false,
            ),
            (
                "receipt",
                "Prévia aguardando confirmação em Revisão · Propostas. Confirme-a lá ou descarte-a para usar esta página.",
                true,
            ),
            ("result", result, true),
            ("error", error, true),
            (
                "expired",
                "Prévia expirada em Revisão · Propostas. Veja o aviso lá ou descarte-o para usar esta página.",
                true,
            ),
        ] {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            match case {
                "busy" => m.busy = true,
                "receipt" => {
                    m.receipt = Some(receipt());
                    m.result = Some(json!({"preview": true}));
                }
                "result" => m.result = Some(json!([])),
                "expired" => {
                    m.receipt = Some(aged(301));
                    m.result = Some(json!({"preview": true}));
                    // O app chama `poll` a cada frame, em qualquer página.
                    m.poll(&shared);
                    assert!(m.receipt.is_none() && m.result.is_none(), "poll expired it");
                }
                _ => m.error = Some("Recusado.".into()),
            }
            let ctx = context(egui::Theme::Light);
            let api = ApiSnapshot::default();
            let mut went = None;
            let mut add = |ui: &mut Ui| {
                if let Some(section) = page(&mut m, ui, &shared, &api, Section::Maintenance) {
                    went = Some(section);
                }
            };
            let out = run(&ctx, vec![], &mut add);
            node(&out, Role::Label, warning);
            for other in [result, error] {
                assert_eq!(
                    nodes(&out, Role::Label, other).len(),
                    usize::from(other == warning),
                    "{case}: one strip, the one of its state"
                );
            }
            assert!(
                nodes(&out, Role::Button, "Manutenção").is_empty(),
                "{case}: the Maintenance form stays closed"
            );
            let discard_button = "Descartar e abrir Manutenção e backup";
            assert_eq!(
                nodes(&out, Role::Button, discard_button).len(),
                usize::from(discard),
                "{case}"
            );
            let go = rect_of(&node(&out, Role::Button, "Ir para Revisão"));
            let discard_rect = nodes(&out, Role::Button, discard_button)
                .first()
                .map(rect_of);
            let discard_at = discard_rect.map(|r| r.center());
            // O botão vem abaixo da faixa inteira, não sobre ela.
            let strip = rect_of(&node(&out, Role::Label, warning));
            if let Some(button) = discard_rect {
                assert!(
                    button.top() > strip.bottom(),
                    "{case}: {button:?} under {strip:?}"
                );
            }
            click(&ctx, go.center(), &mut add);
            drop(add);
            assert_eq!(went, Some(Section::Review), "{case}");
            assert_eq!(m.tab, 0, "{case}: the tab is kept");
            assert!(m.holds(), "{case}: nothing was discarded");
            if let Some(at) = discard_at {
                let mut add = |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Maintenance);
                };
                click(&ctx, at, &mut add);
                let out = run(&ctx, vec![], &mut add);
                drop(add);
                assert_eq!(m.tab, 2, "{case}: discarding opens Maintenance");
                assert!(!m.holds(), "{case}");
                node(&out, Role::Button, "Manutenção");
            }
        }
    }

    #[test]
    fn a_preview_that_expires_on_the_other_page_waits_for_a_click() {
        let waiting = "Prévia aguardando confirmação em Revisão · Propostas. Confirme-a lá ou descarte-a para usar esta página.";
        let expired = "Prévia expirada em Revisão · Propostas. Veja o aviso lá ou descarte-o para usar esta página.";
        let notice = "A prévia expirou após 5 minutos. Prepare a operação novamente.";
        let api = ApiSnapshot::default();
        for go in [true, false] {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.identifier = "draft-id".into();
            m.reason = "draft reason".into();
            m.receipt = Some(receipt());
            m.result = Some(json!({"preview": true}));
            let ctx = context(egui::Theme::Dark);
            // Em Manutenção e backup, a prévia de Revisão ainda vale.
            let out = run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            });
            node(&out, Role::Label, waiting);
            // Passam 5 minutos; o app chama `poll` a cada frame.
            m.receipt.as_mut().unwrap().at = Instant::now() - Duration::from_secs(301);
            m.poll(&shared);
            assert!(m.receipt.is_none(), "go {go}: poll expired the preview");
            assert_eq!(m.notice, notice, "go {go}");
            // O frame seguinte da outra página: a faixa, sem trocar de aba.
            let out = run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            });
            node(&out, Role::Label, expired);
            assert!(
                nodes(&out, Role::Label, waiting).is_empty(),
                "go {go}: not the strip of a live preview"
            );
            assert!(
                nodes(&out, Role::Button, "Manutenção").is_empty(),
                "go {go}: the Maintenance form stays closed"
            );
            assert_eq!(m.tab, 0, "go {go}: the tab is kept");
            assert_eq!(
                (m.identifier.as_str(), m.reason.as_str(), m.notice.as_str()),
                ("draft-id", "draft reason", notice),
                "go {go}: nothing was discarded"
            );
            if go {
                let at = rect_of(&node(&out, Role::Button, "Ir para Revisão")).center();
                let mut went = None;
                click(&ctx, at, |ui| {
                    if let Some(section) = page(&mut m, ui, &shared, &api, Section::Maintenance) {
                        went = Some(section);
                    }
                });
                assert_eq!(went, Some(Section::Review));
                // Em Revisão, o aviso e os campos digitados estão lá.
                let out = run(&ctx, vec![], |ui| {
                    assert_eq!(page(&mut m, ui, &shared, &api, Section::Review), None);
                });
                assert_eq!(m.tab, 0);
                node(&out, Role::Label, notice);
                assert_eq!(nodes(&out, Role::TextInput, "draft-id").len(), 1);
                assert_eq!(nodes(&out, Role::TextInput, "draft reason").len(), 1);
                assert!(
                    nodes(&out, Role::Label, expired).is_empty(),
                    "on its own page the notice is the usual one"
                );
            } else {
                let at = rect_of(&node(
                    &out,
                    Role::Button,
                    "Descartar e abrir Manutenção e backup",
                ))
                .center();
                click(&ctx, at, |ui| {
                    page(&mut m, ui, &shared, &api, Section::Maintenance);
                });
                let out = run(&ctx, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::Maintenance);
                });
                assert_eq!(m.tab, 2, "discarding opens Maintenance");
                assert_eq!(
                    (m.identifier.as_str(), m.reason.as_str(), m.notice.as_str()),
                    ("", "", ""),
                    "the click discarded it"
                );
                assert!(!m.holds());
                node(&out, Role::Button, "Manutenção");
                assert!(nodes(&out, Role::Label, expired).is_empty());
            }
        }
    }

    #[test]
    fn an_operation_answered_while_on_the_other_page_stays_with_its_own_page() {
        // O servidor sintético segura a resposta até o teste liberar.
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let mock = Mock::new(move |r| {
            if r.path.starts_with("/admin/pending-writes/p-1") {
                let _ = gate.lock().unwrap().recv_timeout(PATIENCE);
                Response::json(
                    200,
                    json!({"summary": {"id": "p-1", "title": "Proposta sintética", "status": "pending"}}),
                )
            } else if r.path.starts_with("/admin/pending-writes?") {
                Response::json(200, json!([]))
            } else {
                Response::json(404, json!({}))
            }
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        m.begin(
            &shared,
            Work::Read(Read::Proposal {
                scope: scope(),
                id: "p-1".into(),
            }),
        );
        assert!(m.busy, "precondition: the request is in flight");
        let ctx = context(egui::Theme::Dark);
        let api = ApiSnapshot::default();
        // O usuário abre Manutenção e backup com a consulta em voo.
        let out = run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Maintenance);
        });
        node(
            &out,
            Role::Label,
            "Operação em andamento em Revisão · Propostas. Aguarde a resposta lá; esta página usa o mesmo painel.",
        );
        assert_eq!(m.tab, 0);
        release.send(()).unwrap();
        settle(&mut m, &shared);
        assert!(m.result.is_some(), "the answer was accepted");
        let out = run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Maintenance);
        });
        assert_eq!(m.tab, 0, "the answer does not move to Maintenance");
        node(
            &out,
            Role::Label,
            "Resultado ainda na tela em Revisão · Propostas. Confira-o lá ou descarte-o para usar esta página.",
        );
        assert!(nodes(&out, Role::Label, "title: Proposta sintética").is_empty());
        // De volta a Revisão, o resultado está lá, na aba dele, num cartão.
        let out = run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "title: Proposta sintética");
        node(&out, Role::Label, "Resultado");
        node(&out, Role::Button, "Limpar");
        assert_eq!(m.tab, 0);
        drop(release);
        drop(mock);
    }

    #[test]
    fn clear_dismisses_the_result_the_error_and_the_expired_notice() {
        let notice = "A prévia expirou após 5 minutos. Prepare a operação novamente.";
        for case in ["result", "error", "expired"] {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.tab = 3;
            m.message = "draft".into();
            let shown = match case {
                "result" => {
                    m.result = Some(json!({"saved": true}));
                    m.notice = "Consulta concluída; revise os dados abaixo.".into();
                    "saved: true"
                }
                "error" => {
                    m.error = Some("Recusado.".into());
                    "Recusado."
                }
                _ => {
                    m.receipt = Some(aged(301));
                    m.result = Some(json!({"preview": true}));
                    m.poll(&shared);
                    notice
                }
            };
            assert!(m.holds(), "{case}: precondition");
            let ctx = context(egui::Theme::Dark);
            let api = ApiSnapshot::default();
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            };
            let out = run(&ctx, vec![], &mut add);
            assert_eq!(nodes(&out, Role::Label, shown).len(), 1, "{case}");
            let clear = rect_of(&node(&out, Role::Button, "Limpar"));
            click(&ctx, clear.center(), &mut add);
            let out = run(&ctx, vec![], &mut add);
            drop(add);
            assert!(!m.holds(), "{case}: dismissed");
            assert!(nodes(&out, Role::Label, shown).is_empty(), "{case}");
            assert!(nodes(&out, Role::Button, "Limpar").is_empty(), "{case}");
            assert_eq!(
                (m.tab, m.message.as_str()),
                (3, "draft"),
                "{case}: the form stays"
            );
        }
    }

    #[test]
    fn the_result_clear_button_is_raised_and_never_the_field_color() {
        for (theme, p) in crate::widgets::tests::THEMES {
            for tab in [0, 1, 2, 3] {
                let (_mock, shared, mut m) = quiet();
                m.scope = Some(scope());
                m.tab = tab;
                m.result = Some(json!({"saved": true}));
                let ctx = context(theme);
                let api = ApiSnapshot::default();
                let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::of(tab));
                });
                let case = format!("{theme:?} tab {tab}");
                // O "Limpar" do cartão Resultado.
                node(&out, Role::Label, "Resultado");
                let at = rect_of(&node(&out, Role::Button, "Limpar"));
                let fills: Vec<egui::Color32> = rects(&out)
                    .into_iter()
                    .filter(|r| r.rect == at)
                    .map(|r| r.fill)
                    .collect();
                assert_eq!(fills, [p.raised], "{case}");
                assert_ne!(p.raised, p.field, "{case}: like a field");
            }
        }
    }

    #[test]
    fn the_list_loads_once_per_project_and_state_and_when_the_tab_opens() {
        let mock = server(|_, _| Response::json(200, json!([])));
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope(), other()]);
        let ctx = context(egui::Theme::Dark);
        // Abrir a página com projeto: uma GET, e nenhuma a mais por frame.
        for _ in 0..5 {
            run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
        }
        settle_list(&mut m, &shared);
        assert_eq!((listed(&mock, "demo"), listed(&mock, "other")), (1, 0));
        // Trocar o projeto pelo seletor: exatamente uma GET do novo.
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        choose(
            &ctx,
            [800., 600.],
            &mut add,
            "fixture / demo",
            "fixture / other",
        );
        for _ in 0..5 {
            run(&ctx, vec![], &mut add);
        }
        drop(add);
        settle_list(&mut m, &shared);
        assert_eq!(m.scope, Some(other()));
        assert_eq!((listed(&mock, "demo"), listed(&mock, "other")), (1, 1));
        // Trocar o estado: uma GET com o estado novo.
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        choose(
            &ctx,
            [800., 600.],
            &mut add,
            "Estado: pendentes",
            "Estado: rejeitadas",
        );
        run(&ctx, vec![], &mut add);
        drop(add);
        settle_list(&mut m, &shared);
        let rejected = mock
            .requests()
            .iter()
            .filter(|r| r.path.contains("project=other&status=rejected"))
            .count();
        assert_eq!((listed(&mock, "other"), rejected), (2, 1));
        // Atualizar repete a consulta.
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run(&ctx, vec![], &mut add);
        click(
            &ctx,
            rect_of(&node(&out, Role::Button, "Atualizar")).center(),
            &mut add,
        );
        drop(add);
        settle_list(&mut m, &shared);
        assert_eq!(listed(&mock, "other"), 3);
        // Outra página e de volta: a aba abre de novo e pede a lista.
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Maintenance);
        });
        assert_eq!(m.tab, 2);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        assert_eq!(listed(&mock, "other"), 4);
        // Handoffs não pede a lista.
        m.tab = 1;
        for _ in 0..3 {
            run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
        }
        assert_eq!(listed(&mock, "other"), 4);
    }

    #[test]
    fn a_late_answer_for_the_previous_project_is_ignored() {
        let (release_demo, gate_demo) = mpsc::channel::<()>();
        let (release_other, gate_other) = mpsc::channel::<()>();
        let gates = Mutex::new((gate_demo, gate_other));
        let mock = server(move |project, _| {
            let gates = gates.lock().unwrap();
            let gate = if project == "demo" {
                &gates.0
            } else {
                &gates.1
            };
            let _ = gate.recv_timeout(PATIENCE);
            Response::json(
                200,
                json!([{"id": format!("{project}-1"), "title": format!("Proposta de {project}")}]),
            )
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope(), other()]);
        let ctx = context(egui::Theme::Dark);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        // A consulta de demo está no servidor; o usuário troca para other.
        let until = Instant::now() + PATIENCE;
        while listed(&mock, "demo") == 0 {
            assert!(Instant::now() < until, "demo was never asked");
            std::thread::sleep(Duration::from_millis(2));
        }
        m.scope = Some(other());
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        release_demo.send(()).unwrap();
        // O servidor só atende other depois de responder demo: a resposta de
        // demo já está na fila do painel.
        let until = Instant::now() + PATIENCE;
        while listed(&mock, "other") == 0 {
            assert!(Instant::now() < until, "other was never asked");
            std::thread::sleep(Duration::from_millis(2));
            m.poll(&shared);
        }
        m.poll(&shared);
        assert!(
            m.list.value.is_none(),
            "the answer for demo is ignored: {:?}",
            m.list.value
        );
        let out = run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Carregando propostas…");
        assert!(rows_of(&out).is_empty());
        release_other.send(()).unwrap();
        settle_list(&mut m, &shared);
        let out = run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        assert_eq!(
            rows_of(&out),
            [("Proposta de other · — · —".to_owned(), false)]
        );
        assert_eq!((listed(&mock, "demo"), listed(&mock, "other")), (1, 1));
    }

    #[test]
    fn lists_of_0_1_200_and_201_proposals() {
        for (n, shown) in [(0, 0), (1, 1), (200, 200), (201, 200)] {
            let rows: Vec<Value> = (1..=n).map(proposal).collect();
            let mock = server(move |_, _| Response::json(200, Value::Array(rows.clone())));
            let shared = Shared::unstarted(mock.config());
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            let api = projects(&[scope()]);
            let ctx = context(egui::Theme::Light);
            run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            settle_list(&mut m, &shared);
            let out = run_sized(&ctx, [1320., 860.], vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            assert_eq!(
                m.list
                    .loaded(&(scope(), "pending".into()))
                    .map(<[Value]>::len),
                Some(shown),
                "{n}"
            );
            node(&out, Role::Button, &format!("Propostas {shown}"));
            let rows = rows_of(&out);
            match n {
                0 => assert!(rows.is_empty()),
                1 => assert_eq!(
                    rows,
                    [("Proposta 1 · decisions/p-1.md · 25 min".to_owned(), false)]
                ),
                _ => {
                    // Só as visíveis são montadas; a primeira é a 1.
                    assert!(!rows.is_empty() && rows.len() < 30, "{n}: {}", rows.len());
                    assert_eq!(rows[0].0, "Proposta 1 · decisions/p-1.md · 25 min");
                }
            }
            assert_eq!(
                nodes(&out, Role::Label, "Nenhuma proposta pendente").len(),
                usize::from(n == 0),
                "{n}"
            );
            node(&out, Role::Label, REVIEW_NOTE);
        }
    }

    #[test]
    fn a_list_error_stays_in_the_list_and_never_holds_the_panel() {
        for (status, body, message) in [
            (
                500,
                json!({}),
                "Servidor respondeu HTTP 500. Use Atualizar para tentar de novo.",
            ),
            (
                200,
                json!({"rows": []}),
                "Formato de resposta incompatível. Use Atualizar para tentar de novo.",
            ),
        ] {
            let mock = server(move |_, _| Response::json(status, body.clone()));
            let shared = Shared::unstarted(mock.config());
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            let api = projects(&[scope()]);
            let ctx = context(egui::Theme::Dark);
            run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            settle_list(&mut m, &shared);
            let out = run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            node(&out, Role::Label, "Não foi possível carregar as propostas");
            node(&out, Role::Label, message);
            node(&out, Role::Button, "Propostas —");
            assert!(!m.holds(), "{status}: the list error is not the panel's");
            assert!(m.error.is_none(), "{status}");
            // O resto da página segue: Manutenção abre sem aviso.
            let out = run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            });
            assert_eq!(m.tab, 2, "{status}");
            node(&out, Role::Button, "Manutenção");
        }
    }

    #[test]
    fn the_list_and_the_receipt_never_clear_each_other() {
        let rows = vec![proposal(1), proposal(2)];
        let mock = server(move |_, _| Response::json(200, Value::Array(rows.clone())));
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Dark);
        let key = (scope(), "pending".to_owned());
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        assert_eq!(m.list.loaded(&key).map(<[Value]>::len), Some(2));
        // Uma prévia na folha; a lista é pedida de novo (a aba reabre).
        m.receipt = Some(receipt());
        m.result = Some(json!({"preview": true}));
        m.confirm = "fixture / de".into();
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Maintenance);
        });
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        assert!(
            m.list.value.is_none(),
            "precondition: the list was asked again"
        );
        settle_list(&mut m, &shared);
        assert_eq!(listed(&mock, "demo"), 2);
        assert!(
            m.receipt.is_some() && m.result.is_some(),
            "the list kept the sheet"
        );
        assert_eq!(m.confirm, "fixture / de", "and the typed text");
        // Cancelar a folha e limpar um resultado não tocam na lista.
        m.clear();
        assert_eq!(m.list.loaded(&key).map(<[Value]>::len), Some(2));
        m.result = Some(json!({"ok": true}));
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run(&ctx, vec![], &mut add);
        click(
            &ctx,
            rect_of(&node(&out, Role::Button, "Limpar")).center(),
            &mut add,
        );
        let out = run(&ctx, vec![], &mut add);
        drop(add);
        assert!(m.result.is_none());
        assert_eq!(m.list.loaded(&key).map(<[Value]>::len), Some(2));
        assert_eq!(rows_of(&out).len(), 2, "the rows are still on screen");
        assert_eq!(listed(&mock, "demo"), 2, "and were not asked again");
    }

    #[test]
    fn a_click_on_a_row_opens_its_detail_and_the_actions_use_its_id() {
        let mock = catalog(vec![proposal(1), proposal(2)]);
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Light);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        // Nada escolhido: o detalhe diz como escolher.
        node(&out, Role::Label, "Nenhuma proposta selecionada");
        assert!(rows_of(&out).iter().all(|(_, picked)| !picked));
        let second = rect_of(&node(
            &out,
            Role::Button,
            "Proposta 2 · decisions/p-2.md · 25 min",
        ));
        click_sized(&ctx, WINDOW, second.center(), &mut add);
        drop(add);
        settle_detail(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        assert_eq!(
            rows_of(&out),
            [
                ("Proposta 1 · decisions/p-1.md · 25 min".to_owned(), false),
                ("Proposta 2 · decisions/p-2.md · 25 min".to_owned(), true),
            ]
        );
        node(
            &out,
            Role::Label,
            "Proposta pendente · criada por claude-code há 25 min",
        );
        let title = node(&out, Role::Label, "Proposta 2");
        let list = rect_of(&node(&out, Role::Pane, "Lista de propostas"));
        assert!(
            rect_of(&title).left() > list.right(),
            "the detail is beside the list"
        );
        assert_eq!(nodes(&out, Role::TextInput, "p-2").len(), 1, "the ID field");
        assert!(nodes(&out, Role::Label, "Nenhuma proposta selecionada").is_empty());
        // Aprovar… prepara a prévia da proposta escolhida.
        click_sized(
            &ctx,
            WINDOW,
            rect_of(&node(&out, Role::Button, "Aprovar…")).center(),
            &mut add,
        );
        drop(add);
        assert_eq!(m.identifier, "p-2");
        assert!(m.busy, "the preview was asked");
        settle(&mut m, &shared);
        // A prévia relê a proposta e pede a comparação dela: a 2.
        let asked: Vec<String> = sent(&mock).into_iter().map(|r| r.path).collect();
        assert_eq!(
            read(&mock, "p-2"),
            2,
            "the selection and the preview: {asked:?}"
        );
        assert!(
            asked
                .iter()
                .any(|p| p.starts_with("/admin/pending-writes/p-2/diff?")),
            "{asked:?}"
        );
        // Um ID fora da lista também é lido e mostrado.
        m.clear();
        m.identifier = "p-9".into();
        run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_detail(&mut m, &shared);
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Proposta 9");
        assert_eq!(read(&mock, "p-9"), 1);
        assert!(rows_of(&out).iter().all(|(_, picked)| !picked));
    }

    #[test]
    fn narrow_shows_the_list_or_the_detail_with_back() {
        const NARROW: [f32; 2] = [720., 600.];
        let mock = catalog(vec![proposal(1), proposal(2)]);
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope(), other()]);
        let ctx = context(egui::Theme::Dark);
        run_sized(&ctx, NARROW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let panes = |out: &egui::FullOutput| {
            (
                nodes(out, Role::Pane, "Lista de propostas").len(),
                nodes(out, Role::Pane, "Proposta selecionada").len(),
            )
        };
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (1, 0), "the list alone");
        let row = rect_of(&node(
            &out,
            Role::Button,
            "Proposta 1 · decisions/p-1.md · 25 min",
        ));
        click_sized(&ctx, NARROW, row.center(), &mut add);
        // O detalhe abre no frame seguinte e pede a proposta.
        run_sized(&ctx, NARROW, vec![], &mut add);
        drop(add);
        settle_detail(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (0, 1), "the detail alone");
        node(&out, Role::Label, "Proposta 1");
        let back = rect_of(&node(&out, Role::Button, "← Voltar"));
        click_sized(&ctx, NARROW, back.center(), &mut add);
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (1, 0), "back to the list");
        assert_eq!(
            rows_of(&out)[0],
            ("Proposta 1 · decisions/p-1.md · 25 min".to_owned(), true),
            "the selection is kept"
        );
        // O campo de ID fica no pé da lista, no lugar de "Informar ID…":
        // Enter abre o detalhe do ID digitado, mesmo fora da lista; vazio,
        // não abre.
        assert!(nodes(&out, Role::Button, "Informar ID…").is_empty());
        let field = rect_of(&node(&out, Role::TextInput, "p-1"));
        let list = rect_of(&node(&out, Role::Pane, "Lista de propostas"));
        assert!(field.top() >= list.bottom(), "below the rows: {field:?}");
        click_sized(&ctx, NARROW, field.center(), &mut add);
        let erase = vec![
            key(egui::Key::End),
            key(egui::Key::Backspace),
            key(egui::Key::Backspace),
            key(egui::Key::Backspace),
        ];
        run_sized(&ctx, NARROW, erase, &mut add);
        run_sized(&ctx, NARROW, vec![key(egui::Key::Enter)], &mut add);
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (1, 0), "an empty ID does not open");
        click_sized(&ctx, NARROW, field.center(), &mut add);
        run_sized(
            &ctx,
            NARROW,
            vec![egui::Event::Text("p-9".into())],
            &mut add,
        );
        run_sized(&ctx, NARROW, vec![key(egui::Key::Enter)], &mut add);
        drop(add);
        settle_detail(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (0, 1), "Enter opens the detail");
        node(&out, Role::Label, "Proposta 9");
        node(&out, Role::Button, "← Voltar");
        assert!(
            nodes(&out, Role::TextInput, "p-9").is_empty(),
            "the ID field stays with the list"
        );
        // Outro estado ou outro projeto: de volta à lista.
        for (shown, option) in [
            ("Estado: pendentes", "Estado: aprovadas"),
            ("fixture / demo", "fixture / other"),
        ] {
            let out = run_sized(&ctx, NARROW, vec![], &mut add);
            if let Some(back) = nodes(&out, Role::Button, "← Voltar").first() {
                click_sized(&ctx, NARROW, rect_of(back).center(), &mut add);
            }
            // Uma linha da lista (a nova carrega) abre o detalhe.
            let until = Instant::now() + PATIENCE;
            let out = loop {
                let out = run_sized(&ctx, NARROW, vec![], &mut add);
                if !rows_of(&out).is_empty() {
                    break out;
                }
                assert!(Instant::now() < until, "{option}: the rows never showed");
                std::thread::sleep(Duration::from_millis(2));
            };
            let row = rect_of(&node(
                &out,
                Role::Button,
                "Proposta 1 · decisions/p-1.md · 25 min",
            ));
            click_sized(&ctx, NARROW, row.center(), &mut add);
            let out = run_sized(&ctx, NARROW, vec![], &mut add);
            assert_eq!(panes(&out), (0, 1), "{option}: precondition");
            choose(&ctx, NARROW, &mut add, shown, option);
            // A lista nova carrega (o show faz o poll) e aparece sozinha. O
            // landmark existe já carregando: espera as linhas.
            let until = Instant::now() + PATIENCE;
            let out = loop {
                let out = run_sized(&ctx, NARROW, vec![], &mut add);
                if !rows_of(&out).is_empty() {
                    break out;
                }
                assert!(
                    Instant::now() < until,
                    "{option}: the new list never showed"
                );
                std::thread::sleep(Duration::from_millis(2));
            };
            assert_eq!(panes(&out), (1, 0), "{option}");
        }
        drop(add);
        // Lado a lado não há "Voltar" nem "Informar ID…"; o campo de ID fica
        // no pé da coluna da lista.
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        assert_eq!(panes(&out), (1, 1));
        assert!(nodes(&out, Role::Button, "← Voltar").is_empty());
        assert!(nodes(&out, Role::Button, "Informar ID…").is_empty());
        let list = rect_of(&node(&out, Role::Pane, "Lista de propostas"));
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        let fields: Vec<egui::Rect> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == Role::TextInput)
            .map(|(_, n)| rect_of(n))
            .collect();
        assert!(
            fields
                .iter()
                .any(|f| f.top() >= list.bottom() && f.right() <= list.right()),
            "the ID field under the list: {fields:?} / {list:?}"
        );
    }

    #[test]
    fn narrow_handoffs_show_the_list_or_the_detail_with_back() {
        const NARROW: [f32; 2] = [720., 600.];
        let (_mock, shared, mut m) = quiet();
        m.scope = Some(scope());
        m.tab = 1;
        let now = Utc::now();
        let mut api = projects(&[scope(), other()]);
        api.at = Some(now);
        // Linhas que passam da altura da lista: sem o pé reservado, as
        // últimas pintariam sobre ele.
        api.handoffs = (1..=12)
            .map(|i| Handoff {
                id: format!("h-{i}"),
                agent: "codex".into(),
                at: (now - chrono::Duration::minutes(4)).to_rfc3339(),
                state: "open".into(),
                summary: Some(format!("## Handoff {i}")),
                scope: scope().label(),
                ..Default::default()
            })
            .collect();
        let ctx = context(egui::Theme::Dark);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let panes = |out: &egui::FullOutput| {
            (
                nodes(out, Role::Pane, "Lista de handoffs").len(),
                nodes(out, Role::Pane, "Handoff selecionado").len(),
            )
        };
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (1, 0), "the list alone");
        let inform = rect_of(&node(&out, Role::Button, "Informar ID…"));
        // O pé: os `LIST_FOOT` de baixo da lista, com "Informar ID…".
        let list = rect_of(&node(&out, Role::Pane, "Lista de handoffs"));
        let foot = list.bottom() - LIST_FOOT;
        assert!(
            inform.top() >= foot && inform.bottom() <= list.bottom(),
            "the button in the foot: {inform:?} / {list:?}"
        );
        assert!(
            12. * ROW_HEIGHT > list.height(),
            "precondition: the rows overflow the list"
        );
        // O que cada linha pinta (título, legenda, idade), recortado pela
        // rolagem: nada abaixo do topo do pé.
        let visible: Vec<(String, Rect)> = shown_texts(&out)
            .into_iter()
            .filter(|(t, _)| t.starts_with("Handoff ") || t == "codex" || t == "4 min")
            .collect();
        assert!(
            visible.iter().any(|(t, _)| t == "Handoff 1"),
            "precondition: the rows are painted: {visible:?}"
        );
        for (text, shown) in &visible {
            assert!(
                shown.bottom() <= inform.top(),
                "a row paints over the foot: {text:?} at {shown:?}, foot from {foot}"
            );
        }
        // Com o ID vazio, "Informar ID…" abre o detalhe sem seleção.
        click_sized(&ctx, NARROW, inform.center(), &mut add);
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (0, 1), "Informar ID… opens the detail");
        node(&out, Role::Label, "Nenhum handoff selecionado");
        let back = rect_of(&node(&out, Role::Button, "← Voltar"));
        click_sized(&ctx, NARROW, back.center(), &mut add);
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (1, 0), "back to the list");
        // Um clique na linha abre o detalhe dela, com o ID no campo.
        let row = rect_of(&node(&out, Role::Button, "Handoff 2 · codex · 4 min"));
        click_sized(&ctx, NARROW, row.center(), &mut add);
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (0, 1), "a row opens its detail");
        node(&out, Role::Label, "Handoff 2");
        assert_eq!(nodes(&out, Role::TextInput, "h-2").len(), 1, "the ID field");
        assert!(nodes(&out, Role::Button, "Informar ID…").is_empty());
        let back = rect_of(&node(&out, Role::Button, "← Voltar"));
        click_sized(&ctx, NARROW, back.center(), &mut add);
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        assert_eq!(panes(&out), (1, 0), "back to the list");
        assert!(
            rows_of(&out).contains(&("Handoff 2 · codex · 4 min".to_owned(), true)),
            "the selection is kept"
        );
        node(&out, Role::Button, "Informar ID…");
        drop(add);
        assert_eq!(m.identifier, "h-2");
    }

    #[test]
    fn the_list_landmark_covers_the_list_in_every_state() {
        const NARROW: [f32; 2] = [720., 600.];
        for size in [WINDOW, NARROW] {
            // Propostas: (estado, o que a lista mostra). Carregando segura a
            // resposta até o fim do caso.
            let mut area = None;
            for (state, shown) in [
                ("rows", "Proposta 1 · decisions/p-1.md · 25 min"),
                ("empty", "Nenhuma proposta pendente"),
                ("loading", "Carregando propostas…"),
                ("error", "Não foi possível carregar as propostas"),
            ] {
                let (release, gate) = mpsc::channel::<()>();
                let gate = Mutex::new(gate);
                let mock = server(move |_, _| match state {
                    "rows" => Response::json(200, json!([proposal(1)])),
                    "empty" => Response::json(200, json!([])),
                    "loading" => {
                        let _ = gate.lock().unwrap().recv_timeout(PATIENCE);
                        Response::json(200, json!([]))
                    }
                    _ => Response::json(500, json!({})),
                });
                let shared = Shared::unstarted(mock.config());
                let mut m = Management::new(shared.clone());
                m.scope = Some(scope());
                let api = projects(&[scope()]);
                let ctx = context(egui::Theme::Dark);
                run_sized(&ctx, size, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                if state != "loading" {
                    settle_list(&mut m, &shared);
                }
                let out = run_sized(&ctx, size, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                let case = format!("proposals {state} at {size:?}");
                let role = if state == "rows" {
                    Role::Button
                } else {
                    Role::Label
                };
                assert_eq!(nodes(&out, role, shown).len(), 1, "{case}: precondition");
                let panes = nodes(&out, Role::Pane, "Lista de propostas");
                assert_eq!(panes.len(), 1, "{case}");
                let rect = rect_of(&panes[0]);
                assert_eq!(*area.get_or_insert(rect), rect, "{case}: the same area");
                drop(release);
            }
            // Handoffs: (estado, o que a lista mostra). A lista vem da coleta
            // e não tem erro próprio: com a coleta falha, espera a próxima.
            let now = Utc::now();
            let open = Handoff {
                id: "h-1".into(),
                agent: "codex".into(),
                at: (now - chrono::Duration::minutes(4)).to_rfc3339(),
                state: "open".into(),
                summary: Some("## Handoff 1".into()),
                scope: scope().label(),
                ..Default::default()
            };
            let mut area = None;
            for (state, shown) in [
                ("rows", "Handoff 1 · codex · 4 min"),
                ("empty", "Nenhum handoff aberto"),
                ("loading", "Aguardando a coleta"),
                ("error", "Aguardando a coleta"),
            ] {
                let mut api = projects(&[scope()]);
                match state {
                    "rows" => {
                        api.at = Some(now);
                        api.handoffs = vec![open.clone()];
                    }
                    "empty" => api.at = Some(now),
                    "loading" => {}
                    _ => api.error = Some("Servidor indisponível.".into()),
                }
                let (_mock, shared, mut m) = quiet();
                m.scope = Some(scope());
                m.tab = 1;
                let ctx = context(egui::Theme::Dark);
                let out = run_sized(&ctx, size, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                let case = format!("handoffs {state} at {size:?}");
                let role = if state == "rows" {
                    Role::Button
                } else {
                    Role::Label
                };
                assert_eq!(nodes(&out, role, shown).len(), 1, "{case}: precondition");
                let panes = nodes(&out, Role::Pane, "Lista de handoffs");
                assert_eq!(panes.len(), 1, "{case}");
                let rect = rect_of(&panes[0]);
                assert_eq!(*area.get_or_insert(rect), rect, "{case}: the same area");
            }
        }
    }
    /// Os textos pintados e a parte de cada um que fica à vista (dentro do
    /// recorte da rolagem).
    fn shown_texts(out: &egui::FullOutput) -> Vec<(String, Rect)> {
        fn walk(shape: &egui::Shape, clip: Rect, into: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, clip, into)),
                egui::Shape::Text(t) => {
                    let shown = t.galley.rect.translate(t.pos.to_vec2()).intersect(clip);
                    if shown.is_positive() {
                        into.push((t.galley.text().to_owned(), shown));
                    }
                }
                _ => {}
            }
        }
        let mut all = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, c.clip_rect, &mut all);
        }
        all
    }

    /// Uma tecla apertada, sem modificadores.
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }
    }
    /// O texto `text` pintado em `area`: (cor, tamanho da fonte).
    fn painted(out: &egui::FullOutput, area: Rect, text: &str) -> Vec<(egui::Color32, f32)> {
        crate::widgets::tests::shapes(out)
            .into_iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t)
                    if t.galley.text() == text
                        && area.contains_rect(t.galley.rect.translate(t.pos.to_vec2())) =>
                {
                    let format = &t.galley.job.sections.first()?.format;
                    let color = t
                        .override_text_color
                        .or(Some(format.color).filter(|&c| c != egui::Color32::PLACEHOLDER))
                        .unwrap_or(t.fallback_color);
                    Some((color, format.font_id.size))
                }
                _ => None,
            })
            .collect()
    }
    /// Espera até `done`, sondando o painel.
    fn wait(m: &mut Management, shared: &Shared, what: &str, done: impl Fn() -> bool) {
        let until = Instant::now() + PATIENCE;
        while !done() {
            assert!(Instant::now() < until, "{what}");
            std::thread::sleep(Duration::from_millis(2));
            m.poll(shared);
        }
    }

    #[test]
    fn each_selection_reads_its_detail_once_and_shows_it() {
        for (theme, p) in crate::widgets::tests::THEMES {
            let mock = catalog(vec![proposal(1), proposal(2)]);
            let shared = Shared::unstarted(mock.config());
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            let api = projects(&[scope()]);
            let ctx = context(theme);
            run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            settle_list(&mut m, &shared);
            let reads = |mock: &Mock| {
                mock.requests()
                    .iter()
                    .filter(|r| detail_of(&r.path).is_some())
                    .count()
            };
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            node(&out, Role::Label, "Nenhuma proposta selecionada");
            let row = rect_of(&node(
                &out,
                Role::Button,
                "Proposta 2 · decisions/p-2.md · 25 min",
            ))
            .center();
            for _ in 0..3 {
                run_sized(&ctx, WINDOW, vec![], &mut add);
            }
            assert_eq!(reads(&mock), 0, "{theme:?}: nothing selected, nothing read");
            click_sized(&ctx, WINDOW, row, &mut add);
            drop(add);
            settle_detail(&mut m, &shared);
            // Mais frames e outro clique na mesma linha: a mesma seleção,
            // nenhum pedido novo (a senha conta os pedidos).
            let asked = m.detail.ticket;
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            for _ in 0..5 {
                run_sized(&ctx, WINDOW, vec![], &mut add);
            }
            click_sized(&ctx, WINDOW, row, &mut add);
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            drop(add);
            assert_eq!(m.detail.ticket, asked, "{theme:?}: the same selection");
            assert_eq!(
                (read(&mock, "p-1"), read(&mock, "p-2")),
                (0, 1),
                "{theme:?}: one GET for the selection"
            );
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            // A linha de estado (12, text2), o título (18), a justificativa
            // e a evidência (text2, como no artboard).
            let meta = "Proposta pendente · criada por claude-code há 25 min";
            let evidence = "“expirou” — sessions/a.md";
            let detail = rect_of(&node(&out, Role::Pane, "Proposta selecionada"));
            for (text, want) in [
                (meta, (p.text2, 12.)),
                ("Proposta 2", (p.text, 18.)),
                ("Justificativa da proposta 2.", (p.text2, 13.)),
                ("Evidências", (p.text2, 12.)),
                (evidence, (p.text2, 13.)),
            ] {
                node(&out, Role::Label, text);
                assert_eq!(painted(&out, detail, text), [want], "{theme:?}: {text}");
            }
            assert!(
                nodes(&out, Role::Label, "Proposta não carregada").is_empty(),
                "{theme:?}"
            );
            // O ID digitado: nada é lido enquanto se digita; Enter lê uma vez.
            let field = rect_of(&node(&out, Role::TextInput, "p-2")).center();
            click_sized(&ctx, WINDOW, field, &mut add);
            run_sized(
                &ctx,
                WINDOW,
                vec![
                    key(egui::Key::End),
                    key(egui::Key::Backspace),
                    key(egui::Key::Backspace),
                    key(egui::Key::Backspace),
                ],
                &mut add,
            );
            for typed in ["p", "-", "1"] {
                run_sized(
                    &ctx,
                    WINDOW,
                    vec![egui::Event::Text(typed.into())],
                    &mut add,
                );
            }
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            assert_eq!(
                nodes(&out, Role::TextInput, "p-1").len(),
                1,
                "{theme:?}: precondition, typed"
            );
            node(&out, Role::Label, "Proposta não carregada");
            assert!(
                nodes(&out, Role::Label, "Proposta 2").is_empty(),
                "{theme:?}"
            );
            drop(add);
            assert_eq!(
                (m.detail.ticket, reads(&mock)),
                (asked, 1),
                "{theme:?}: nothing asked while typing"
            );
            run_sized(&ctx, WINDOW, vec![key(egui::Key::Enter)], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            assert_eq!(m.detail.ticket, asked + 1, "{theme:?}: Enter asks once");
            settle_detail(&mut m, &shared);
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            node(&out, Role::Label, "Proposta 1");
            assert_eq!(
                (read(&mock, "p-1"), read(&mock, "p-2"), reads(&mock)),
                (1, 1, 2),
                "{theme:?}: one GET per selection"
            );
        }
    }

    #[test]
    fn the_typed_id_is_read_when_tab_or_a_click_elsewhere_leaves_the_field() {
        // Enter tem o seu teste (acima); aqui, os outros dois jeitos de sair
        // do campo.
        for way in ["Tab", "click"] {
            let mock = catalog(vec![proposal(1), proposal(2)]);
            let shared = Shared::unstarted(mock.config());
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            // O motivo separa o campo dele do de ID, vazio.
            m.reason = "motivo".into();
            let api = projects(&[scope()]);
            let ctx = context(egui::Theme::Light);
            run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            settle_list(&mut m, &shared);
            let asked = m.detail.ticket;
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            let field = rect_of(&node(&out, Role::TextInput, "")).center();
            // Um ponto vazio do detalhe, longe do campo e das linhas.
            let elsewhere = rect_of(&node(&out, Role::Pane, "Proposta selecionada")).center();
            click_sized(&ctx, WINDOW, field, &mut add);
            for typed in ["p", "-", "1"] {
                run_sized(
                    &ctx,
                    WINDOW,
                    vec![egui::Event::Text(typed.into())],
                    &mut add,
                );
            }
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            drop(add);
            assert_eq!(
                focused(&out).as_deref(),
                Some("p-1"),
                "{way}: precondition, typing in the field"
            );
            assert_eq!(m.detail.ticket, asked, "{way}: nothing asked while typing");
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            match way {
                "Tab" => {
                    run_sized(&ctx, WINDOW, vec![key(egui::Key::Tab)], &mut add);
                }
                _ => click_sized(&ctx, WINDOW, elsewhere, &mut add),
            }
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            drop(add);
            assert_ne!(
                focused(&out).as_deref(),
                Some("p-1"),
                "{way}: left the field"
            );
            assert_eq!(m.detail.ticket, asked + 1, "{way}: leaving asks once");
            assert_eq!(
                m.detail.key,
                Some((scope(), "p-1".to_owned())),
                "{way}: the typed ID"
            );
            settle_detail(&mut m, &shared);
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            node(&out, Role::Label, "Proposta 1");
            assert_eq!(read(&mock, "p-1"), 1, "{way}: one GET");
        }
    }

    #[test]
    fn a_late_detail_for_the_previous_selection_is_ignored() {
        let (release_1, gate_1) = mpsc::channel::<()>();
        let (release_2, gate_2) = mpsc::channel::<()>();
        let gates = Mutex::new((gate_1, gate_2));
        let mock = Mock::new(move |r| {
            if r.path.starts_with("/admin/pending-writes?") {
                return Response::json(200, json!([proposal(1), proposal(2)]));
            }
            match detail_of(&r.path) {
                Some(i) => {
                    let gates = gates.lock().unwrap();
                    let gate = if i == 1 { &gates.0 } else { &gates.1 };
                    let _ = gate.recv_timeout(PATIENCE);
                    Response::json(200, proposal_detail(i))
                }
                None => Response::json(404, json!({})),
            }
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Dark);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        let rows: Vec<egui::Pos2> = [1, 2]
            .map(|i| {
                rect_of(&node(
                    &out,
                    Role::Button,
                    &format!("Proposta {i} · decisions/p-{i}.md · 25 min"),
                ))
                .center()
            })
            .into();
        click_sized(&ctx, WINDOW, rows[0], &mut add);
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        node(&out, Role::Label, "Carregando proposta…");
        // A 1 está no servidor; o usuário escolhe a 2.
        click_sized(&ctx, WINDOW, rows[1], &mut add);
        drop(add);
        release_1.send(()).unwrap();
        // O servidor só atende a 2 depois de responder a 1: a resposta da 1
        // já está na fila do painel.
        wait(&mut m, &shared, "p-2 was never asked", || {
            read(&mock, "p-2") == 1
        });
        m.poll(&shared);
        assert!(
            m.detail.value.is_none(),
            "the answer for p-1 is ignored: {:?}",
            m.detail.value
        );
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Carregando proposta…");
        assert!(nodes(&out, Role::Label, "Proposta 1").is_empty());
        release_2.send(()).unwrap();
        settle_detail(&mut m, &shared);
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Proposta 2");
        assert!(nodes(&out, Role::Label, "Proposta 1").is_empty());
        assert_eq!((read(&mock, "p-1"), read(&mock, "p-2")), (1, 1));
    }

    #[test]
    fn a_detail_error_shows_a_strip_and_never_holds_the_panel() {
        for (status, body, message) in [
            (
                404,
                json!({}),
                "Recurso ausente, desabilitado ou incompatível com esta versão.",
            ),
            (500, json!({}), "Servidor respondeu HTTP 500."),
            // A resposta de outra proposta.
            (
                200,
                json!({"summary": {"id": "p-7", "title": "Outra"}}),
                "Formato de resposta incompatível.",
            ),
        ] {
            for (theme, p) in crate::widgets::tests::THEMES {
                let body = body.clone();
                let mock = Mock::new(move |r| {
                    if r.path.starts_with("/admin/pending-writes?") {
                        Response::json(200, json!([proposal(1)]))
                    } else if detail_of(&r.path).is_some() {
                        Response::json(status, body.clone())
                    } else {
                        Response::json(404, json!({}))
                    }
                });
                let shared = Shared::unstarted(mock.config());
                let mut m = Management::new(shared.clone());
                m.scope = Some(scope());
                let api = projects(&[scope()]);
                let ctx = context(theme);
                run(&ctx, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                settle_list(&mut m, &shared);
                let mut add = |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                };
                let out = run_sized(&ctx, WINDOW, vec![], &mut add);
                let row = rect_of(&node(
                    &out,
                    Role::Button,
                    "Proposta 1 · decisions/p-1.md · 25 min",
                ));
                click_sized(&ctx, WINDOW, row.center(), &mut add);
                drop(add);
                settle_detail(&mut m, &shared);
                let asked = m.detail.ticket;
                let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                let case = format!("{status} {theme:?}");
                assert_eq!(m.detail.ticket, asked, "{case}: no retry by itself");
                let strip = rect_of(&node(
                    &out,
                    Role::Label,
                    &format!("Não foi possível carregar a proposta. {message}"),
                ));
                assert!(
                    rects(&out).iter().any(
                        |r| r.fill == widgets::tone(p.bad, 0.08) && r.rect.contains_rect(strip)
                    ),
                    "{case}: a Bad strip"
                );
                let detail = rect_of(&node(&out, Role::Pane, "Proposta selecionada"));
                assert!(detail.contains_rect(strip), "{case}: in the detail");
                assert!(nodes(&out, Role::Label, "Outra").is_empty(), "{case}");
                assert!(!m.holds(), "{case}: the detail error is not the panel's");
                assert!(m.error.is_none() && m.result.is_none(), "{case}");
                assert_eq!(rows_of(&out).len(), 1, "{case}: the list stays");
                assert!(
                    !node(&out, Role::Button, "Aprovar…").is_disabled(),
                    "{case}"
                );
                assert_eq!(read(&mock, "p-1"), 1, "{case}");
            }
        }
    }

    #[test]
    fn the_meta_line_drops_what_the_detail_lacks() {
        let now = DateTime::parse_from_rfc3339("2026-03-15T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let micros = |at: &str| DateTime::parse_from_rfc3339(at).unwrap().timestamp_micros();
        let staged = |agent: Value| json!({"event": "staged", "actor_json": {"agent": agent}});
        let detail = |summary: Value, events: Value| json!({"summary": summary, "events": events});
        let pending = |at: Value| json!({"id": "p-1", "status": "pending", "staged_at": at});
        let ago = json!(micros("2026-03-15T11:35:00Z"));
        let by = json!([staged(json!("claude-code"))]);
        let cases: Vec<(&str, Value, Option<&str>)> = vec![
            (
                "everything",
                detail(pending(ago.clone()), by.clone()),
                Some("Proposta pendente · criada por claude-code há 25 min"),
            ),
            (
                "no status",
                detail(json!({"staged_at": ago.clone()}), by.clone()),
                Some("Proposta · criada por claude-code há 25 min"),
            ),
            (
                "unknown status",
                detail(
                    json!({"status": "weird", "staged_at": ago.clone()}),
                    by.clone(),
                ),
                Some("Proposta weird · criada por claude-code há 25 min"),
            ),
            (
                "no events",
                detail(pending(ago.clone()), Value::Null),
                Some("Proposta pendente · criada há 25 min"),
            ),
            (
                "no staged event: the approver is not the author",
                detail(
                    pending(ago.clone()),
                    json!([{"event": "approved", "actor_json": {"agent": "revisor"}}]),
                ),
                Some("Proposta pendente · criada há 25 min"),
            ),
            (
                "staged after another event",
                detail(
                    pending(ago.clone()),
                    json!([
                        {"event": "approved", "actor_json": {"agent": "revisor"}},
                        staged(json!("codex")),
                    ]),
                ),
                Some("Proposta pendente · criada por codex há 25 min"),
            ),
            (
                "no agent",
                detail(
                    pending(ago.clone()),
                    json!([{"event": "staged", "actor_json": {}}]),
                ),
                Some("Proposta pendente · criada há 25 min"),
            ),
            (
                "blank agent",
                detail(pending(ago.clone()), json!([staged(json!(" "))])),
                Some("Proposta pendente · criada há 25 min"),
            ),
            (
                "agent not a string",
                detail(pending(ago.clone()), json!([staged(json!(7))])),
                Some("Proposta pendente · criada há 25 min"),
            ),
            (
                "no staged_at",
                detail(json!({"status": "pending"}), by.clone()),
                Some("Proposta pendente · criada por claude-code"),
            ),
            (
                "unreadable staged_at",
                detail(pending(json!("ontem")), by.clone()),
                Some("Proposta pendente · criada por claude-code"),
            ),
            (
                "only the status",
                detail(json!({"status": "rejected"}), Value::Null),
                Some("Proposta rejeitada"),
            ),
            ("nothing", json!({}), None),
            ("not an object", json!("p-1"), None),
            ("null", Value::Null, None),
        ];
        for (case, v, want) in cases {
            assert_eq!(proposal_meta(&v, now).as_deref(), want, "{case}");
        }
        // A idade: `staged_at` em microssegundos, ou RFC 3339.
        for (at, want) in [
            (json!(micros("2026-03-15T11:59:30Z")), "agora"),
            (json!(micros("2026-03-15T11:35:00Z")), "há 25 min"),
            (json!(micros("2026-03-15T09:00:00Z")), "há 3 h"),
            (json!(micros("2026-01-02T12:00:00Z")), "em 02/01"),
            (json!(micros("2025-01-02T12:00:00Z")), "em 02/01/25"),
            (json!("2026-03-15T11:35:00Z"), "há 25 min"),
        ] {
            assert_eq!(
                proposal_meta(&detail(pending(at.clone()), by.clone()), now).as_deref(),
                Some(format!("Proposta pendente · criada por claude-code {want}").as_str()),
                "{at}"
            );
        }
    }

    #[test]
    fn a_detail_without_fields_shows_only_what_it_has() {
        let (_mock, shared, mut m) = quiet();
        m.scope = Some(scope());
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Light);
        let full = proposal_detail(1);
        // (resposta, o que aparece, o que não)
        let cases: [(Value, Vec<&str>, Vec<&str>); 4] = [
            (
                json!({"summary": {"id": "p-1"}}),
                vec![],
                vec![
                    "Proposta",
                    "Proposta 1",
                    "Justificativa da proposta 1.",
                    "Evidências",
                ],
            ),
            (
                json!({"summary": {"id": "p-1", "title": " "}, "rationale": "", "evidence_json": []}),
                vec![],
                vec!["Proposta", " ", "", "Evidências"],
            ),
            (
                json!({
                    "summary": {"id": "p-1", "title": "Só título"},
                    "evidence_json": [
                        {"quote": "sem página"},
                        {"page": "sem-citação.md"},
                        {},
                        "texto solto",
                    ],
                }),
                vec!["Só título", "Evidências", "“sem página”", "sem-citação.md"],
                vec!["“texto solto”", "texto solto"],
            ),
            (
                full,
                vec![
                    "Proposta 1",
                    "Justificativa da proposta 1.",
                    "Evidências",
                    "“expirou” — sessions/a.md",
                ],
                vec![],
            ),
        ];
        for (n, (detail, shown, absent)) in cases.into_iter().enumerate() {
            m.identifier = "p-1".into();
            m.detail.key = Some((scope(), "p-1".into()));
            m.detail.value = Some(Ok(detail));
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            for text in shown {
                node(&out, Role::Label, text);
            }
            for text in absent {
                assert!(
                    nodes(&out, Role::Label, text).is_empty(),
                    "case {n}: {text}"
                );
            }
            // Sem nada a mostrar, nenhum aviso de estado no lugar.
            for state in [
                "Carregando proposta…",
                "Proposta não carregada",
                "Nenhuma proposta selecionada",
            ] {
                assert!(
                    nodes(&out, Role::Label, state).is_empty(),
                    "case {n}: {state}"
                );
            }
            node(&out, Role::TextInput, "p-1");
        }
    }

    #[test]
    fn the_detail_and_the_list_the_receipt_and_the_result_never_clear_each_other() {
        let mock = catalog(vec![proposal(1), proposal(2)]);
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Dark);
        let key = (scope(), "pending".to_owned());
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        // Um resultado na tela: a seleção não o apaga, nem a lista.
        m.result = Some(json!({"ok": true}));
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        let row = rect_of(&node(
            &out,
            Role::Button,
            "Proposta 2 · decisions/p-2.md · 25 min",
        ));
        click_sized(&ctx, WINDOW, row.center(), &mut add);
        drop(add);
        settle_detail(&mut m, &shared);
        let shown = |m: &Management| {
            m.detail
                .value
                .as_ref()
                .and_then(|v| v.as_ref().ok())
                .map(|v| v["summary"]["id"].clone())
        };
        assert_eq!(shown(&m), Some(json!("p-2")));
        assert_eq!(m.result, Some(json!({"ok": true})), "the result stays");
        assert_eq!(m.list.loaded(&key).map(<[Value]>::len), Some(2));
        assert_eq!(listed(&mock, "demo"), 1, "the list was not asked again");
        // Limpar o resultado não apaga o detalhe.
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        node(&out, Role::Label, "Proposta 2");
        click_sized(
            &ctx,
            WINDOW,
            rect_of(&node(&out, Role::Button, "Limpar")).center(),
            &mut add,
        );
        drop(add);
        assert!(m.result.is_none(), "precondition: cleared");
        assert_eq!(shown(&m), Some(json!("p-2")), "Limpar keeps the detail");
        // Outro estado pede só a lista (Atualizar relê as duas) e não apaga
        // o detalhe; nem a volta ao estado anterior.
        for (shown_state, option) in [
            ("Estado: pendentes", "Estado: aprovadas"),
            ("Estado: aprovadas", "Estado: pendentes"),
        ] {
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            choose(&ctx, WINDOW, &mut add, shown_state, option);
            drop(add);
            assert!(
                m.list.value.is_none(),
                "{option}: precondition, the list was asked"
            );
            assert_eq!(
                shown(&m),
                Some(json!("p-2")),
                "{option}: the list keeps the detail"
            );
            settle_list(&mut m, &shared);
            assert_eq!(shown(&m), Some(json!("p-2")), "{option}");
        }
        // Uma resposta do detalhe com a folha aberta: a folha, o resultado e
        // o texto digitado ficam.
        m.receipt = Some(receipt());
        m.result = Some(json!({"preview": true}));
        m.confirm = "fixture / de".into();
        m.detail.request(&shared, (scope(), "p-1".into()));
        settle_detail(&mut m, &shared);
        assert_eq!(shown(&m), Some(json!("p-1")), "precondition: answered");
        assert!(m.receipt.is_some(), "the detail kept the sheet");
        assert_eq!(m.result, Some(json!({"preview": true})));
        assert_eq!(m.confirm, "fixture / de");
        assert_eq!(m.list.loaded(&key).map(<[Value]>::len), Some(2));
        // Cancelar a folha não apaga o detalhe.
        m.clear();
        assert_eq!(shown(&m), Some(json!("p-1")), "the sheet keeps the detail");
        assert_eq!((read(&mock, "p-1"), read(&mock, "p-2")), (1, 1));
    }

    #[test]
    fn another_project_drops_the_detail_and_a_new_selection_reads_again() {
        let mock = catalog(vec![proposal(1), proposal(2)]);
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope(), other()]);
        let ctx = context(egui::Theme::Light);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        let row = "Proposta 2 · decisions/p-2.md · 25 min";
        for (round, (from, to)) in [
            ("fixture / demo", "fixture / other"),
            ("fixture / other", "fixture / demo"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            click_sized(
                &ctx,
                WINDOW,
                rect_of(&node(&out, Role::Button, row)).center(),
                &mut add,
            );
            drop(add);
            settle_detail(&mut m, &shared);
            assert_eq!(
                read(&mock, "p-2"),
                round + 1,
                "{round}: one GET per selection"
            );
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            choose(&ctx, WINDOW, &mut add, from, to);
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            drop(add);
            assert!(
                m.detail.key.is_none() && m.detail.value.is_none(),
                "{round}"
            );
            node(&out, Role::Label, "Nenhuma proposta selecionada");
            assert!(nodes(&out, Role::Label, "Proposta 2").is_empty(), "{round}");
            settle_list(&mut m, &shared);
        }
        assert_eq!(read(&mock, "p-2"), 2);
    }

    /// Servidor sintético para as releituras: a lista são `rows` e o detalhe
    /// de `p-N` traz no título a ordem da leitura entre todas as do detalhe
    /// ("Proposta N (leitura K)"); as escritas (POST) respondem `post`.
    fn rereading(rows: Vec<Value>, post: u16) -> Mock {
        let reads = AtomicUsize::new(0);
        Mock::new(move |r| {
            if r.path.starts_with("/admin/pending-writes?") {
                Response::json(200, Value::Array(rows.clone()))
            } else if let Some(i) = detail_of(&r.path) {
                let k = reads.fetch_add(1, SeqCst) + 1;
                let mut v = proposal_detail(i);
                v["summary"]["title"] = json!(format!("Proposta {i} (leitura {k})"));
                Response::json(200, v)
            } else if r.method == "POST" {
                Response::json(post, json!({"ok": post == 200}))
            } else {
                Response::json(404, json!({}))
            }
        })
    }
    /// As GETs de detalhe e de lista que o servidor recebeu.
    fn reads(mock: &Mock) -> (usize, usize) {
        let requests = mock.requests();
        let detail = requests.iter().filter(|r| detail_of(&r.path).is_some());
        let list = requests
            .iter()
            .filter(|r| r.path.starts_with("/admin/pending-writes?"));
        (detail.count(), list.count())
    }

    #[test]
    fn the_actions_and_the_fields_stay_put_while_the_detail_loads() {
        let long = "Aumentar o TTL do cache de cotações para quinze minutos, invalidado no fechamento do pregão e revisado a cada trimestre pela equipe";
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let mock = Mock::new(move |r| {
            if r.path.starts_with("/admin/pending-writes?") {
                return Response::json(200, json!([proposal(1), proposal(2), proposal(3)]));
            }
            match detail_of(&r.path) {
                Some(3) => Response::json(500, json!({})),
                Some(i) => {
                    let _ = gate.lock().unwrap().recv_timeout(PATIENCE);
                    let mut v = proposal_detail(i);
                    v["summary"]["title"] = json!(long);
                    v["rationale"] = json!(long.repeat(6));
                    Response::json(200, v)
                }
                None => Response::json(404, json!({})),
            }
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        m.reason = "motivo".into();
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Dark);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        // Os botões e os campos (o de ID pelo valor) de um frame.
        let anchors = |out: &egui::FullOutput, id: &str| {
            let at = |role, text: &str| rect_of(&node(out, role, text));
            [
                at(Role::Button, "Rejeitar…"),
                at(Role::Button, "Aprovar…"),
                at(Role::Button, "Ver detalhes"),
                at(Role::TextInput, id),
                at(Role::TextInput, "motivo"),
            ]
        };
        let row = |out: &egui::FullOutput, i: usize| {
            rect_of(&node(
                out,
                Role::Button,
                &format!("Proposta {i} · decisions/p-{i}.md · 25 min"),
            ))
            .center()
        };
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        let blank = anchors(&out, "");
        node(&out, Role::Label, "Nenhuma proposta selecionada");
        let pane = rect_of(&node(&out, Role::Pane, "Proposta selecionada"));
        let list = rect_of(&node(&out, Role::Pane, "Lista de propostas"));
        click_sized(&ctx, WINDOW, row(&out, 2), &mut add);
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        node(&out, Role::Label, "Carregando proposta…");
        let loading = anchors(&out, "p-2");
        drop(add);
        release.send(()).unwrap();
        settle_detail(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        let title = rect_of(&node(&out, Role::Label, long));
        let loaded = anchors(&out, "p-2");
        click_sized(&ctx, WINDOW, row(&out, 3), &mut add);
        drop(add);
        settle_detail(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        node(
            &out,
            Role::Label,
            "Não foi possível carregar a proposta. Servidor respondeu HTTP 500.",
        );
        let failed = anchors(&out, "p-3");
        assert_eq!(loading, blank, "nothing selected → loading");
        assert_eq!(loaded, blank, "loading → loaded");
        assert_eq!(failed, blank, "→ error");
        // A linha do título: as ações à direita do detalhe, a 28 da borda; o
        // campo de ID no pé da coluna da lista.
        let [reject, approve, details, field, reason] = blank;
        assert!(
            (approve.right() - (pane.right() - 28.)).abs() < 0.5,
            "{approve:?} in {pane:?}"
        );
        assert!(
            reject.right() < approve.left() && reject.top() == approve.top(),
            "{reject:?} / {approve:?}"
        );
        assert!(
            approve.top() < pane.top() + 22. + 40.,
            "at the top: {approve:?}"
        );
        for f in [field, details] {
            assert!(
                f.top() >= list.bottom() && f.right() <= list.right(),
                "{f:?} under {list:?}"
            );
        }
        assert!(reason.top() > pane.center().y, "at the foot: {reason:?}");
        assert!(
            title.height() > 30. && title.right() < reject.left(),
            "a wrapped title beside the actions: {title:?}"
        );
        drop(add);
        // Tela estreita: no detalhe, as ações e o motivo também ficam.
        const NARROW: [f32; 2] = [720., 600.];
        let narrow = |out: &egui::FullOutput| {
            let at = |role, text: &str| rect_of(&node(out, role, text));
            [
                at(Role::Button, "← Voltar"),
                at(Role::Button, "Rejeitar…"),
                at(Role::Button, "Aprovar…"),
                at(Role::TextInput, "motivo"),
            ]
        };
        m.open = false;
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        click_sized(&ctx, NARROW, row(&out, 1), &mut add);
        let out = run_sized(&ctx, NARROW, vec![], &mut add);
        node(&out, Role::Label, "Carregando proposta…");
        let before = narrow(&out);
        assert!(
            nodes(&out, Role::TextInput, "p-1").is_empty(),
            "the ID field stays with the list"
        );
        drop(add);
        release.send(()).unwrap();
        settle_detail(&mut m, &shared);
        let out = run_sized(&ctx, NARROW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, long);
        let [back, reject, approve, _] = before;
        assert!(
            reject.top() == approve.top() && approve.top() > back.bottom(),
            "the actions under ← Voltar: {back:?} {approve:?}"
        );
        assert_eq!(narrow(&out), before, "narrow: loading → loaded");
    }

    /// Aprovar… e Rejeitar… de um frame: habilitados como `enabled`, no
    /// mesmo lugar que em `places` (o 1º frame o grava) e, desabilitados,
    /// com a dica de pendente sob o ponteiro.
    fn approve_and_reject(
        ctx: &egui::Context,
        add: &mut impl FnMut(&mut Ui),
        places: &mut Option<[Rect; 2]>,
        case: &str,
        enabled: bool,
    ) {
        let out = run_sized(ctx, WINDOW, vec![], &mut *add);
        let buttons = ["Aprovar…", "Rejeitar…"].map(|b| node(&out, Role::Button, b));
        for button in &buttons {
            assert_eq!(
                button.is_disabled(),
                !enabled,
                "{case}: {:?} should be enabled = {enabled}",
                button.label()
            );
        }
        let rects = buttons.each_ref().map(rect_of);
        assert_eq!(
            *places.get_or_insert(rects),
            rects,
            "{case}: the buttons moved"
        );
        // O egui não mostra a dica logo depois de um clique: o ponteiro sai,
        // espera e volta parado sobre Aprovar….
        run_sized(ctx, WINDOW, vec![egui::Event::PointerGone], &mut *add);
        for _ in 0..10 {
            run_sized(ctx, WINDOW, vec![], &mut *add);
        }
        let point = vec![egui::Event::PointerMoved(rects[0].center())];
        let mut out = run_sized(ctx, WINDOW, point, &mut *add);
        for _ in 0..45 {
            out = run_sized(ctx, WINDOW, vec![], &mut *add);
        }
        assert_eq!(
            nodes(&out, Role::Label, NOT_PENDING).len(),
            usize::from(!enabled),
            "{case}: the hint"
        );
    }

    #[test]
    fn approve_and_reject_disable_in_place_when_the_known_state_is_not_pending() {
        let mut places = None;
        // (estado da proposta, habilitados?); `None` = sem o campo.
        for (state, enabled) in [
            (Some("pending"), true),
            (Some("approved"), false),
            (Some("rejected"), false),
            (Some("failed"), false),
            (Some("conflict"), false),
            (None, true),
        ] {
            let with_state = move |mut v: Value| {
                match state {
                    Some(state) => v["status"] = json!(state),
                    None => {
                        v.as_object_mut().unwrap().remove("status");
                    }
                }
                v
            };
            // O detalhe só sai com a porteira aberta; o p-8 falha.
            let open = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let gate = open.clone();
            let mock = Mock::new(move |r| {
                if r.path.starts_with("/admin/pending-writes?") {
                    return Response::json(
                        200,
                        json!([with_state(proposal(1)), with_state(proposal(2))]),
                    );
                }
                if r.path.starts_with("/admin/pending-writes/p-8?") {
                    return Response::json(500, json!({}));
                }
                match detail_of(&r.path) {
                    Some(i) => {
                        let until = Instant::now() + PATIENCE;
                        while !gate.load(SeqCst) && Instant::now() < until {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        let mut v = proposal_detail(i);
                        v["summary"] = with_state(v["summary"].take());
                        Response::json(200, v)
                    }
                    None => Response::json(404, json!({})),
                }
            });
            let shared = Shared::unstarted(mock.config());
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            // O filtro do estado, como na tela ("Estado: aprovadas"…).
            if let Some(state) = state {
                m.status = state.into();
            }
            let api = projects(&[scope()]);
            let ctx = context(egui::Theme::Dark);
            run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            settle_list(&mut m, &shared);
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            let row = rect_of(&node(
                &out,
                Role::Button,
                "Proposta 1 · decisions/p-1.md · 25 min",
            ));
            click_sized(&ctx, WINDOW, row.center(), &mut add);
            // Enquanto o detalhe não chega, vale a linha da lista.
            let case = format!("{state:?}, the row");
            approve_and_reject(&ctx, &mut add, &mut places, &case, enabled);
            drop(add);
            open.store(true, SeqCst);
            settle_detail(&mut m, &shared);
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            node(&out, Role::Label, "Proposta 1");
            let case = format!("{state:?}, the detail");
            approve_and_reject(&ctx, &mut add, &mut places, &case, enabled);
            drop(add);
            // Um ID fora da lista: desconhecido até o detalhe chegar.
            open.store(false, SeqCst);
            m.identifier = "p-9".into();
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            node(&out, Role::Label, "Carregando proposta…");
            let case = format!("{state:?}, an ID outside the list, loading");
            approve_and_reject(&ctx, &mut add, &mut places, &case, true);
            drop(add);
            open.store(true, SeqCst);
            settle_detail(&mut m, &shared);
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let case = format!("{state:?}, an ID outside the list, loaded");
            approve_and_reject(&ctx, &mut add, &mut places, &case, enabled);
            drop(add);
            // Com erro, também desconhecido.
            m.identifier = "p-8".into();
            run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            settle_detail(&mut m, &shared);
            assert!(
                matches!(m.detail.value, Some(Err(_))),
                "precondition: {:?}",
                m.detail.value
            );
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let case = format!("{state:?}, an ID outside the list, error");
            approve_and_reject(&ctx, &mut add, &mut places, &case, true);
        }
    }

    #[test]
    fn refresh_rereads_the_list_and_the_selected_detail() {
        let mock = rereading(vec![proposal(1), proposal(2)], 200);
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Light);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        // Sem seleção: só a lista.
        let tickets = |m: &Management| (m.list.ticket, m.detail.ticket);
        let refresh = |m: &mut Management| {
            let mut add = |ui: &mut Ui| {
                page(m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            let at = rect_of(&node(&out, Role::Button, "Atualizar")).center();
            click_sized(&ctx, WINDOW, at, &mut add);
        };
        let before = tickets(&m);
        refresh(&mut m);
        settle_list(&mut m, &shared);
        assert_eq!(tickets(&m), (before.0 + 1, before.1), "no selection");
        assert_eq!(reads(&mock), (0, 2));
        // Com uma proposta escolhida: a lista e ela.
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        let row = rect_of(&node(
            &out,
            Role::Button,
            "Proposta 2 · decisions/p-2.md · 25 min",
        ));
        click_sized(&ctx, WINDOW, row.center(), &mut add);
        drop(add);
        settle_detail(&mut m, &shared);
        let before = tickets(&m);
        refresh(&mut m);
        assert_eq!(tickets(&m), (before.0 + 1, before.1 + 1), "both asked");
        settle_list(&mut m, &shared);
        settle_detail(&mut m, &shared);
        assert_eq!(reads(&mock), (2, 3));
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Proposta 2 (leitura 2)");
        assert!(nodes(&out, Role::Label, "Proposta 2 (leitura 1)").is_empty());
    }

    #[test]
    fn a_successful_approve_or_reject_rereads_the_list_and_the_detail_and_nothing_else_does() {
        let reject = || Intent::Reject {
            id: "p-1".into(),
            reason: "duplicada".into(),
        };
        let approve = || Intent::Approve { id: "p-1".into() };
        let checkpoint = || Intent::Checkpoint {
            message: "test".into(),
        };
        // (intenção, o botão final, a resposta da escrita, relê?)
        let cases = [
            (approve(), "Aprovar", 200, true),
            (reject(), "Rejeitar", 200, true),
            (approve(), "Aprovar", 500, false),
            (reject(), "Rejeitar", 409, false),
            (checkpoint(), "Criar checkpoint", 200, false),
        ];
        for (intent, verb, status, reread) in cases {
            let case = format!("{verb} {status}");
            let mock = rereading(vec![proposal(1)], status);
            let shared = Shared::unstarted(mock.config());
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            let api = projects(&[scope()]);
            let ctx = context(egui::Theme::Dark);
            run(&ctx, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            settle_list(&mut m, &shared);
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            let row = rect_of(&node(
                &out,
                Role::Button,
                "Proposta 1 · decisions/p-1.md · 25 min",
            ));
            click_sized(&ctx, WINDOW, row.center(), &mut add);
            drop(add);
            settle_detail(&mut m, &shared);
            m.receipt = Some(Receipt {
                action: Action {
                    scope: scope(),
                    intent,
                },
                generation: 0,
                at: Instant::now(),
            });
            m.result = Some(json!({"evidencia": "prévia"}));
            let before = (m.list.ticket, m.detail.ticket);
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = frames(&ctx, &mut add);
            let confirm = rect_of(&node(&out, Role::Button, verb)).center();
            click_sized(&ctx, WINDOW, confirm, &mut add);
            drop(add);
            assert!(m.busy, "{case}: precondition, applying");
            settle(&mut m, &shared);
            assert_eq!(m.error.is_none(), status == 200, "{case}: {:?}", m.error);
            if status == 200 {
                // A releitura é automática: o aviso não manda atualizar.
                assert_eq!(
                    m.notice, "Resposta recebida. Confira os detalhes abaixo.",
                    "{case}"
                );
            }
            let asked = usize::from(reread);
            assert_eq!(
                (m.list.ticket, m.detail.ticket),
                (before.0 + asked as u64, before.1 + asked as u64),
                "{case}: reread {reread}"
            );
            settle_list(&mut m, &shared);
            settle_detail(&mut m, &shared);
            std::thread::sleep(Duration::from_millis(30));
            m.poll(&shared);
            assert_eq!(reads(&mock), (1 + asked, 1 + asked), "{case}");
            // Depois da escrita, só leituras.
            let requests = mock.requests();
            let post = requests
                .iter()
                .position(|r| r.method == "POST")
                .expect("the write");
            assert!(
                requests[post + 1..].iter().all(|r| r.method == "GET"),
                "{case}: only reads after the write"
            );
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            node(
                &out,
                Role::Label,
                &format!("Proposta 1 (leitura {})", 1 + asked),
            );
        }
    }

    #[test]
    fn a_read_in_flight_before_the_reread_never_overwrites_it() {
        // a: a aprovação; b: a 2ª leitura do detalhe (Atualizar); c: a 3ª,
        // a releitura depois da aprovação.
        let (release_a, gate_a) = mpsc::channel::<()>();
        let (release_b, gate_b) = mpsc::channel::<()>();
        let (release_c, gate_c) = mpsc::channel::<()>();
        let gates = Mutex::new([gate_a, gate_b, gate_c]);
        let count = AtomicUsize::new(0);
        let mock = Mock::new(move |r| {
            let wait = |n: usize| {
                let _ = gates.lock().unwrap()[n].recv_timeout(PATIENCE);
            };
            if r.path.starts_with("/admin/pending-writes?") {
                Response::json(200, json!([proposal(1)]))
            } else if let Some(i) = detail_of(&r.path) {
                let k = count.fetch_add(1, SeqCst) + 1;
                if k > 1 {
                    wait(k - 1);
                }
                let mut v = proposal_detail(i);
                v["summary"]["title"] = json!(format!("Proposta {i} (leitura {k})"));
                Response::json(200, v)
            } else if r.path.starts_with("/admin/pending-writes/p-1/approve?") {
                wait(0);
                Response::json(200, json!({"ok": true}))
            } else {
                Response::json(404, json!({}))
            }
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Dark);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        let row = rect_of(&node(
            &out,
            Role::Button,
            "Proposta 1 · decisions/p-1.md · 25 min",
        ));
        click_sized(&ctx, WINDOW, row.center(), &mut add);
        drop(add);
        settle_detail(&mut m, &shared);
        m.receipt = Some(Receipt {
            action: Action {
                scope: scope(),
                intent: Intent::Approve { id: "p-1".into() },
            },
            generation: 0,
            at: Instant::now(),
        });
        m.result = Some(json!({"evidencia": "prévia"}));
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = frames(&ctx, &mut add);
        click_sized(
            &ctx,
            WINDOW,
            rect_of(&node(&out, Role::Button, "Aprovar")).center(),
            &mut add,
        );
        drop(add);
        // Com a aprovação no servidor, Atualizar pede a leitura 2. O
        // servidor sintético atende um pedido por vez: a leitura 2 na frente
        // prenderia a aprovação atrás da porteira dela (os dois saem de
        // trabalhadores diferentes, sem ordem garantida).
        wait(
            &mut m,
            &shared,
            "the approval never reached the server",
            || {
                mock.requests()
                    .iter()
                    .any(|r| r.path.starts_with("/admin/pending-writes/p-1/approve?"))
            },
        );
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        click_sized(
            &ctx,
            WINDOW,
            rect_of(&node(&out, Role::Button, "Atualizar")).center(),
            &mut add,
        );
        drop(add);
        assert!(m.busy, "precondition: the approval is in flight");
        let second = m.detail.ticket;
        release_a.send(()).unwrap();
        settle(&mut m, &shared);
        let third = m.detail.ticket;
        // A leitura 2 responde depois do novo pedido: é ignorada.
        release_b.send(()).unwrap();
        wait(&mut m, &shared, "the third read never came", || {
            reads(&mock).0 == 3
        });
        m.poll(&shared);
        assert!(
            m.detail.value.is_none(),
            "the second read is ignored: {:?}",
            m.detail.value
        );
        release_c.send(()).unwrap();
        settle_detail(&mut m, &shared);
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Proposta 1 (leitura 3)");
        assert!(nodes(&out, Role::Label, "Proposta 1 (leitura 2)").is_empty());
        assert_eq!(
            third,
            second + 1,
            "the approval asked again, with a new ticket"
        );
    }

    #[test]
    fn typing_a_handoff_id_that_matches_an_open_one_keeps_the_focus() {
        let (_mock, shared, mut m) = quiet();
        m.scope = Some(scope());
        m.tab = 1;
        let now = Utc::now();
        let mut api = projects(&[scope()]);
        api.at = Some(now);
        api.handoffs = vec![Handoff {
            id: "h-12".into(),
            agent: "codex".into(),
            at: (now - chrono::Duration::minutes(4)).to_rfc3339(),
            state: "open".into(),
            summary: Some("## Primeiro".into()),
            scope: scope().label(),
            ..Default::default()
        }];
        for size in [WINDOW, [720., 600.]] {
            m.identifier.clear();
            m.open = true;
            let ctx = context(egui::Theme::Light);
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, size, vec![], &mut add);
            node(&out, Role::Label, "Nenhum handoff selecionado");
            let field = rect_of(&node(&out, Role::TextInput, ""));
            click_sized(&ctx, size, field.center(), &mut add);
            // Em "h-12" o detalhe troca o aviso pelo handoff, acima do campo.
            for typed in ["h", "-", "1", "2", "x"] {
                run_sized(&ctx, size, vec![egui::Event::Text(typed.into())], &mut add);
            }
            drop(add);
            assert_eq!(m.identifier, "h-12x", "{size:?}: the focus stayed");
        }
    }

    #[test]
    fn handoffs_are_the_open_ones_of_the_project_in_the_snapshot() {
        let (mock, shared, mut m) = quiet();
        m.scope = Some(scope());
        m.tab = 1;
        let now = Utc::now();
        let handoff = |id: &str, scope: &Scope, summary: &str| Handoff {
            id: id.into(),
            agent: "codex".into(),
            at: (now - chrono::Duration::minutes(4)).to_rfc3339(),
            state: "open".into(),
            summary: Some(summary.into()),
            scope: scope.label(),
            ..Default::default()
        };
        let mut api = projects(&[scope(), other()]);
        // Antes da 1ª coleta: aguardando.
        let ctx = context(egui::Theme::Light);
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Aguardando a coleta");
        node(&out, Role::Button, "Handoffs —");
        api.at = Some(now);
        api.handoffs = vec![
            handoff("h-1", &scope(), "## Primeiro\n\nCorpo do primeiro."),
            handoff("h-2", &other(), "## De outro projeto"),
            handoff("h-3", &scope(), "## Terceiro"),
        ];
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        node(&out, Role::Button, "Handoffs 2");
        assert_eq!(
            rows_of(&out),
            [
                (
                    "Primeiro · codex · Corpo do primeiro. · 4 min".to_owned(),
                    false
                ),
                ("Terceiro · codex · 4 min".to_owned(), false),
            ]
        );
        let first = rect_of(&node(
            &out,
            Role::Button,
            "Primeiro · codex · Corpo do primeiro. · 4 min",
        ));
        click_sized(&ctx, WINDOW, first.center(), &mut add);
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        node(&out, Role::Label, "Primeiro");
        node(&out, Role::Label, "Corpo do primeiro.");
        click_sized(
            &ctx,
            WINDOW,
            rect_of(&node(&out, Role::Button, "Cancelar handoff…")).center(),
            &mut add,
        );
        drop(add);
        assert_eq!(m.identifier, "h-1");
        assert!(m.busy, "the preview was asked");
        settle(&mut m, &shared);
        assert!(
            !sent(&mock).is_empty(),
            "the cancel preview asked the server"
        );
        // Zero abertos no projeto, depois da coleta.
        api.handoffs.retain(|h| h.scope == other().label());
        m.clear();
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Nenhum handoff aberto");
        node(&out, Role::Button, "Handoffs 0");
    }

    #[test]
    fn manage_in_review_prefills_the_project_and_the_handoff_unless_held() {
        let api = {
            let mut api = projects(&[scope(), other()]);
            api.at = Some(Utc::now());
            api.handoffs = vec![Handoff {
                id: "h-7".into(),
                agent: "codex".into(),
                summary: Some("## Sétimo".into()),
                scope: other().label(),
                ..Default::default()
            }];
            api
        };
        // (painel antes, o pedido muda o painel?)
        for (case, moves) in [
            ("free on Proposals", true),
            ("free on Handoffs", true),
            ("free on Maintenance", true),
            ("busy", false),
            ("result", false),
            ("error", false),
            ("expired", false),
            ("receipt", false),
            ("result on Maintenance", false),
        ] {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.identifier = "typed".into();
            match case {
                "free on Handoffs" => m.tab = 1,
                "free on Maintenance" => m.tab = 2,
                "busy" => m.busy = true,
                "result" => m.result = Some(json!({})),
                "error" => m.error = Some("x".into()),
                "expired" => m.expired = true,
                "receipt" => {
                    m.receipt = Some(receipt());
                    m.result = Some(json!({}));
                }
                "result on Maintenance" => {
                    m.tab = 3;
                    m.result = Some(json!({}));
                }
                _ => {}
            }
            let before = (m.tab, m.picked().0, m.identifier.clone());
            m.open_handoff(&api.projects, "fixture / other", "h-7");
            if moves {
                assert_eq!(m.tab, 1, "{case}");
                assert_eq!(
                    m.picked(),
                    (Some("fixture / other".into()), "h-7"),
                    "{case}"
                );
                let ctx = context(egui::Theme::Dark);
                let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                node(&out, Role::Label, "Sétimo");
                assert_eq!(rows_of(&out), [("Sétimo · codex · —".to_owned(), true)]);
                node(&out, Role::ComboBox, "fixture / other");
            } else {
                assert_eq!(
                    (m.tab, m.picked().0, m.identifier.clone()),
                    before,
                    "{case}: nothing changes"
                );
            }
        }
        // Mesmo projeto e aba: só o handoff muda; o resto do formulário fica.
        let (_mock, _shared, mut m) = quiet();
        m.scope = Some(other());
        m.tab = 1;
        m.days = 30;
        m.reason = "kept".into();
        m.open_handoff(&api.projects, "fixture / other", "h-7");
        assert_eq!(m.picked(), (Some("fixture / other".into()), "h-7"));
        assert_eq!((m.days, m.reason.as_str()), (30, "kept"));
        // Um projeto que saiu da lista: Handoffs, sem projeto nem ID.
        let (_mock, shared, mut m) = quiet();
        m.scope = Some(scope());
        m.identifier = "typed".into();
        m.open_handoff(&api.projects, "fixture / gone", "h-1");
        assert_eq!((m.tab, m.picked()), (1, (None, "")));
        let ctx = context(egui::Theme::Dark);
        let out = run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "Selecione o projeto para continuar.");
        node(&out, Role::ComboBox, "Selecione um projeto");
    }

    #[test]
    fn the_backup_mode_is_a_segmented_that_speaks_and_switches_its_choice() {
        for (theme, _) in crate::widgets::tests::THEMES {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.tab = 3;
            let api = projects(&[scope()]);
            let ctx = context(theme);
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            };
            let modes = ["Backup global", "Exportar projeto (OKF)"];
            let state =
                |out: &egui::FullOutput| modes.map(|mode| node(out, Role::Button, mode).toggled());
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            let on = Some(Toggled::True);
            let off = Some(Toggled::False);
            assert_eq!(state(&out), [on, off], "{theme:?}");
            for mode in modes {
                assert!(
                    nodes(&out, Role::RadioButton, mode).is_empty(),
                    "{theme:?}: {mode} is not an egui radio"
                );
            }
            let at = modes.map(|mode| rect_of(&node(&out, Role::Button, mode)).center());
            click_sized(&ctx, WINDOW, at[1], &mut add);
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            assert_eq!(state(&out), [off, on], "{theme:?}: OKF");
            click_sized(&ctx, WINDOW, at[0], &mut add);
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            assert_eq!(state(&out), [on, off], "{theme:?}: back to the backup");
            click_sized(&ctx, WINDOW, at[1], &mut add);
            drop(add);
            assert!(m.okf, "{theme:?}: the mode follows the choice");
        }
    }

    #[test]
    fn maintenance_forms_are_cards_with_26_px_fields_and_the_app_checkbox() {
        for (theme, p) in crate::widgets::tests::THEMES {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.tab = 3;
            let ctx = context(theme);
            let api = ApiSnapshot::default();
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            });
            for title in [
                "Backup e exportação",
                "Checkpoints da wiki",
                "Restaurar uma página",
            ] {
                node(&out, Role::Label, title);
            }
            let fields: Vec<Node> = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == Role::TextInput)
                .map(|(_, n)| n.clone())
                .collect();
            assert_eq!(fields.len(), 4, "{theme:?}");
            for field in &fields {
                assert_eq!(rect_of(field).height(), 26., "{theme:?}");
            }
            let tick = rect_of(&node(
                &out,
                Role::CheckBox,
                "Usar checkpoint Git (desmarcado: ID da versão da página)",
            ));
            assert!(
                rects(&out).iter().any(|r| r.rect.width() == 14.
                    && r.rect.height() == 14.
                    && r.stroke.color == p.text2
                    && tick.contains_rect(r.rect)),
                "{theme:?}: the 14 px box with the text2 border"
            );
            // A tarefa de embeddings mostra a caixa de regenerar.
            m.tab = 2;
            m.task = Task::Embeddings;
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            });
            let tick = rect_of(&node(
                &out,
                Role::CheckBox,
                "Regenerar também embeddings já existentes",
            ));
            assert!(
                rects(&out).iter().any(|r| r.rect.width() == 14.
                    && r.stroke.color == p.text2
                    && tick.contains_rect(r.rect)),
                "{theme:?}"
            );
            node(&out, Role::ComboBox, "Gerar embeddings");
        }
    }

    /// Onze frames sem ponteiro: a barra de rolagem aparece com uma animação
    /// curta (1/60 s por frame).
    fn settled(ctx: &egui::Context, add: &mut impl FnMut(&mut Ui)) -> egui::FullOutput {
        let mut out = run_sized(ctx, WINDOW, vec![], &mut *add);
        for _ in 0..10 {
            out = run_sized(ctx, WINDOW, vec![], &mut *add);
        }
        out
    }

    #[test]
    fn the_details_and_the_result_show_their_scroll_bars_without_hover_only_when_long() {
        use crate::widgets::tests::scroll_handles;
        for (theme, p) in crate::widgets::tests::THEMES {
            for long in [false, true] {
                let n = if long { 60 } else { 1 };
                let case = format!("{theme:?} long {long}");
                let ctx = context(theme);
                // Propostas: o corpo do detalhe, entre o título e o motivo.
                let (_mock, shared, mut m) = quiet();
                m.scope = Some(scope());
                m.identifier = "p-1".into();
                let mut v = proposal_detail(1);
                v["evidence_json"] = (0..n)
                    .map(|i| json!({"page": format!("sessions/{i}.md"), "quote": "expirou"}))
                    .collect();
                m.detail.key = Some((scope(), "p-1".into()));
                m.detail.value = Some(Ok(v));
                let api = projects(&[scope()]);
                let out = settled(&ctx, &mut |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                let pane = rect_of(&node(&out, Role::Pane, "Proposta selecionada"));
                let handles = scroll_handles(&out, pane.shrink2(vec2(28., 22.)), p.text2);
                assert_eq!(handles.len(), usize::from(long), "{case}: proposal");
                // Handoffs: o detalhe do selecionado.
                let (_mock, shared, mut m) = quiet();
                m.scope = Some(scope());
                m.tab = 1;
                m.identifier = "h-1".into();
                let mut api = projects(&[scope()]);
                api.at = Some(Utc::now());
                api.handoffs = vec![Handoff {
                    id: "h-1".into(),
                    agent: "codex".into(),
                    at: Utc::now().to_rfc3339(),
                    state: "open".into(),
                    summary: Some(
                        (0..n)
                            .map(|i| format!("Parágrafo {i}."))
                            .collect::<Vec<_>>()
                            .join("\n\n"),
                    ),
                    scope: scope().label(),
                    ..Default::default()
                }];
                let out = settled(&ctx, &mut |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                let pane = rect_of(&node(&out, Role::Pane, "Handoff selecionado"));
                let handles = scroll_handles(&out, pane.shrink2(vec2(28., 22.)), p.text2);
                assert_eq!(handles.len(), usize::from(long), "{case}: handoff");
                // O resultado de uma operação, no cartão do topo.
                let (_mock, shared, mut m) = quiet();
                m.scope = Some(scope());
                m.tab = 2;
                // Uma palavra só, longa: as linhas vão até a borda do texto.
                m.result = Some(Value::Object(
                    (0..n)
                        .map(|i| (format!("campo {i:02}"), json!("a".repeat(300))))
                        .collect(),
                ));
                let api = projects(&[scope()]);
                let out = settled(&ctx, &mut |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Maintenance);
                });
                let title = rect_of(&node(&out, Role::Label, "Resultado"));
                let card = rects(&out)
                    .into_iter()
                    .find(|r| r.fill == p.surf && r.rect.contains_rect(title))
                    .expect("the result card")
                    .rect;
                // A margem do cartão: borda 1 e 16.
                let handles = scroll_handles(&out, card.shrink(17.), p.text2);
                assert_eq!(handles.len(), usize::from(long), "{case}: result");
                // O texto para a `DETAIL_GAP` da borda, fora da barra (e
                // chega perto disso: o cenário enche as linhas).
                let edge = card.right() - 17. - DETAIL_GAP;
                let right = painted_rects(&out, card, "campo ")
                    .iter()
                    .map(|r| r.right())
                    .fold(0., f32::max);
                assert!(
                    right <= edge + 0.5 && right > edge - 10.,
                    "{case}: the result ends at {right}, not by {edge}"
                );
            }
        }
    }
    /// Os retângulos dos textos pintados em `area` que começam com `prefix`.
    fn painted_rects(out: &egui::FullOutput, area: Rect, prefix: &str) -> Vec<Rect> {
        crate::widgets::tests::shapes(out)
            .into_iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text().starts_with(prefix) => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                _ => None,
            })
            .filter(|r| area.contains_rect(*r))
            .collect()
    }

    #[test]
    fn a_label_that_fills_its_column_wraps_8_short_of_the_field() {
        // Uma palavra só, longa: as linhas enchem a coluna do rótulo.
        let label = "a".repeat(60);
        let ctx = context(egui::Theme::Dark);
        let out = run(&ctx, vec![], |ui| {
            labeled_line(ui, &label, |ui| {
                widgets::text_field(ui, &mut String::new(), 300., "", false, 10);
            });
        });
        let field = rects(&out)
            .into_iter()
            .find(|r| r.rect.height() == 26. && r.rect.width() == 300.)
            .expect("the field")
            .rect;
        let shown: Vec<Rect> = painted_rects(&out, Rect::EVERYTHING, &label);
        let [at] = shown[..] else {
            panic!("one label, got {shown:?}");
        };
        assert!(at.height() > 26., "precondition: it wraps: {at:?}");
        assert!(
            field.left() - at.right() >= 8. && field.left() - at.right() < 8. + 10.,
            "{at:?} by {field:?}"
        );
    }

    #[test]
    fn at_1320_the_backup_labels_are_whole_and_clear_of_their_fields() {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.tab = 3;
            let ctx = context(theme);
            let api = ApiSnapshot::default();
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            });
            let fields: Vec<Rect> = out
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, n)| n.role() == Role::TextInput)
                .map(|(_, n)| rect_of(n))
                .collect();
            for label in [
                "Arquivo de destino (caminho absoluto)",
                "Descrição do checkpoint",
                "Caminho da página",
                "ID completo da versão ou checkpoint",
            ] {
                let shown: Vec<(Rect, bool)> = crate::widgets::tests::shapes(&out)
                    .into_iter()
                    .filter_map(|s| match s {
                        egui::Shape::Text(t) if t.galley.text() == label => {
                            Some((t.galley.rect.translate(t.pos.to_vec2()), t.galley.elided))
                        }
                        _ => None,
                    })
                    .collect();
                let [(at, elided)] = shown[..] else {
                    panic!("{theme:?}: one {label:?}, got {shown:?}");
                };
                assert!(!elided, "{theme:?}: {label:?} is cut");
                // O campo da mesma linha, à direita do rótulo.
                let field = fields
                    .iter()
                    .find(|f| f.y_range().contains(at.center().y) && f.left() > at.left())
                    .unwrap_or_else(|| panic!("{theme:?}: the field of {label:?}"));
                assert!(
                    field.left() - at.right() >= 8.,
                    "{theme:?}: {label:?} at {at:?} touches {field:?}"
                );
            }
        }
    }

    #[test]
    fn the_proposal_and_handoff_ids_are_typed_in_mono_12() {
        for (tab, id) in [(0, "p-1"), (1, "h-1")] {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.tab = tab;
            m.identifier = id.into();
            let ctx = context(egui::Theme::Dark);
            let api = projects(&[scope()]);
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            let field = rect_of(&node(&out, Role::TextInput, id));
            let fonts: Vec<FontId> = crate::widgets::tests::shapes(&out)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t)
                        if t.galley.text() == id
                            && field.contains_rect(t.galley.rect.translate(t.pos.to_vec2())) =>
                    {
                        Some(t.galley.job.sections.first()?.format.font_id.clone())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(fonts, [FontId::monospace(12.)], "tab {tab}");
        }
    }

    #[test]
    fn the_id_fields_hint_the_exact_id_and_stop_a_paste_at_128_characters() {
        for (theme, p) in crate::widgets::tests::THEMES {
            for tab in [0, 1] {
                let (_mock, shared, mut m) = quiet();
                m.scope = Some(scope());
                m.tab = tab;
                let ctx = context(theme);
                let api = projects(&[scope()]);
                let mut add = |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                };
                let out = run_sized(&ctx, WINDOW, vec![], &mut add);
                let case = format!("{theme:?} tab {tab}");
                // A dica da `text_field`: 12 proporcional, em text2.
                let hints: Vec<(Rect, FontId, egui::Color32)> = crate::widgets::tests::shapes(&out)
                    .into_iter()
                    .filter_map(|s| match s {
                        egui::Shape::Text(t) if t.galley.text() == "ID exato" => {
                            let format = &t.galley.job.sections.first()?.format;
                            Some((
                                t.galley.rect.translate(t.pos.to_vec2()),
                                format.font_id.clone(),
                                format.color,
                            ))
                        }
                        _ => None,
                    })
                    .collect();
                let [(at, ref font, color)] = hints[..] else {
                    panic!("{case}: one hint, got {hints:?}");
                };
                assert_eq!(
                    (font.clone(), color),
                    (FontId::proportional(12.), p.text2),
                    "{case}"
                );
                // Colado no campo da dica: o que passa de 128 é cortado.
                click_sized(&ctx, WINDOW, at.center(), &mut add);
                run_sized(
                    &ctx,
                    WINDOW,
                    vec![egui::Event::Paste("a".repeat(200))],
                    &mut add,
                );
                drop(add);
                assert_eq!(m.identifier.chars().count(), 128, "{case}");
            }
        }
    }

    #[test]
    fn the_age_field_is_a_26_px_field_with_the_line_border() {
        for (theme, p) in crate::widgets::tests::THEMES {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.tab = 1;
            let ctx = context(theme);
            let api = projects(&[scope()]);
            let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            let at = rect_of(&node(&out, Role::SpinButton, "7 dias"));
            assert_eq!(at.height(), 26., "{theme:?}");
            let boxes: Vec<_> = rects(&out)
                .into_iter()
                .filter(|r| r.rect == at)
                .map(|r| (r.fill, r.stroke, r.corner_radius))
                .collect();
            assert_eq!(
                boxes,
                [(
                    p.field,
                    Stroke::new(1.0_f32, p.line),
                    egui::CornerRadius::same(5)
                )],
                "{theme:?}: like the text fields"
            );
        }
    }

    #[test]
    fn the_project_picker_widens_into_what_the_line_leaves() {
        // ~60 caracteres: bem mais que os 260 de antes.
        let long = Scope {
            workspace: "default".into(),
            project: "acme-servicos-portal-coleta-zero-carbono-v2-e-mais-um-tanto".into(),
        };
        // Um nome que não cabe em linha nenhuma.
        let huge = Scope {
            workspace: "default".into(),
            project: "um-projeto".repeat(30),
        };
        // (janela, projeto, cabe inteiro?, ações na mesma linha?)
        for (size, project, whole, beside) in [
            (WINDOW, &long, true, true),
            ([720., 600.], &long, true, false),
            (WINDOW, &huge, false, true),
        ] {
            let (_mock, shared, mut m) = quiet();
            m.scope = Some(project.clone());
            m.tab = 1;
            let ctx = context(egui::Theme::Dark);
            let api = projects(std::slice::from_ref(project));
            let out = run_sized(&ctx, size, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            let case = format!("{size:?} {}", project.project.len());
            let label = project.label();
            let picker = rect_of(&node(&out, Role::ComboBox, &label));
            let elided: Vec<bool> = crate::widgets::tests::shapes(&out)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) if t.galley.text() == label => Some(t.galley.elided),
                    _ => None,
                })
                .collect();
            assert_eq!(elided, [!whole], "{case}: {picker:?}");
            assert!(picker.width() > PROJECT_WIDTH, "{case}: {picker:?}");
            let tabs = rect_of(&segment(&out, "Propostas"));
            let expire = rect_of(&node(&out, Role::Button, "Expirar antigos…"));
            assert!(picker.right() + space::S <= tabs.left() + 0.5, "{case}");
            let line = expire.top() < picker.bottom();
            assert_eq!(line, beside, "{case}: {picker:?} {expire:?}");
            if beside {
                let segmented = rects(&out)
                    .into_iter()
                    .map(|r| r.rect)
                    .filter(|r| r.contains_rect(tabs) && r.height() <= 30.)
                    .fold(tabs, |a, b| a.union(b));
                assert!(
                    segmented.right() + space::S <= expire.left(),
                    "{case}: {segmented:?} {expire:?}"
                );
            }
        }
    }

    #[test]
    fn maintenance_and_backup_preview_buttons_are_secondary_and_sweep_stays_destructive() {
        // (aba, tarefa, botões e se são destrutivos)
        type Buttons<'a> = &'a [(&'a str, bool)];
        let cases: [(u8, Task, Buttons); 3] = [
            (
                2,
                Task::Lint,
                &[
                    ("Preparar prévia sem aplicar", false),
                    ("Preparar prévia de reconsolidação", false),
                    ("Revisar feedback", false),
                ],
            ),
            (2, Task::Sweep, &[("Preparar prévia sem aplicar", true)]),
            (
                3,
                Task::Lint,
                &[
                    ("Revisar download privado", false),
                    ("Listar últimos 100 checkpoints", false),
                    ("Revisar criação de checkpoint", false),
                    ("Preparar restauração", false),
                ],
            ),
        ];
        for (theme, p) in crate::widgets::tests::THEMES {
            for (tab, task, buttons) in cases {
                let (_mock, shared, mut m) = quiet();
                m.scope = Some(scope());
                m.tab = tab;
                m.task = task;
                let ctx = context(theme);
                let api = ApiSnapshot::default();
                let out = run_sized(&ctx, WINDOW, vec![], |ui| {
                    page(&mut m, ui, &shared, &api, Section::Maintenance);
                });
                for &(label, destructive) in buttons {
                    let at = rect_of(&node(&out, Role::Button, label));
                    let case = format!("{theme:?} {label}");
                    let fills: Vec<_> = rects(&out)
                        .into_iter()
                        .filter(|r| r.rect == at)
                        .map(|r| (r.fill, r.stroke))
                        .collect();
                    // Sobre o cartão surf, ao lado de campos: raised (no
                    // escuro, o bg seria o do campo).
                    assert_eq!(
                        fills,
                        [(p.raised, Stroke::new(1.0_f32, p.line))],
                        "{case}: raised"
                    );
                    let want = if destructive { p.bad } else { p.text };
                    assert_eq!(painted(&out, at, label), [(want, 13.)], "{case}");
                    assert_ne!(p.raised, p.field, "{case}: like a field");
                }
                // O verde fica com o botão final da folha.
                assert!(
                    rects(&out).iter().all(|r| r.fill != p.acc),
                    "{theme:?} tab {tab}: a green button"
                );
            }
        }
    }

    #[test]
    fn the_handoff_actions_expire_with_the_age_and_load_the_history() {
        let mock = Mock::new(|r| {
            if r.path.contains("/handoffs?") {
                Response::json(200, json!({"handoffs": [{"id": "h-1", "state": "open"}]}))
            } else {
                Response::json(404, json!({}))
            }
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        m.tab = 1;
        m.days = 30;
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Dark);
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        node(&out, Role::SpinButton, "30 dias");
        let expire = rect_of(&node(&out, Role::Button, "Expirar antigos…"));
        click_sized(&ctx, WINDOW, expire.center(), &mut add);
        drop(add);
        settle(&mut m, &shared);
        assert_eq!(
            m.receipt.as_ref().map(|r| &r.action.intent),
            Some(&Intent::ExpireHandoffs { days: 30 })
        );
        m.clear();
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        let history = rect_of(&node(&out, Role::Button, "Carregar histórico"));
        click_sized(&ctx, WINDOW, history.center(), &mut add);
        drop(add);
        settle(&mut m, &shared);
        assert!(m.receipt.is_none(), "a read, not a preview");
        assert!(m.result.is_some());
        let paths: Vec<String> = mock.requests().into_iter().map(|r| r.path).collect();
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(paths[0].contains("state=open"), "{paths:?}");
        assert!(
            paths[1].contains("/handoffs?") && !paths[1].contains("state="),
            "the history has every state: {paths:?}"
        );
    }

    #[test]
    fn the_detail_actions_use_the_selected_id_and_the_reason() {
        let mock = Mock::new(|r| {
            if r.path.starts_with("/admin/pending-writes?") {
                Response::json(200, json!([proposal(1), proposal(2)]))
            } else if r.path.starts_with("/admin/pending-writes/p-2/diff?") {
                Response::json(200, json!({"proposal_id": "p-2", "diff": "x"}))
            } else if r.path.starts_with("/admin/pending-writes/p-2?") {
                Response::json(200, json!({"summary": {"id": "p-2", "status": "pending"}}))
            } else {
                Response::json(404, json!({}))
            }
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        m.identifier = "p-2".into();
        m.reason = "duplicada".into();
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Light);
        run(&ctx, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        settle_list(&mut m, &shared);
        // O ID no campo é a seleção: o detalhe dele chega antes de medir os
        // botões, que descem quando ele aparece.
        settle_detail(&mut m, &shared);
        // Ver detalhes: a leitura vai ao cartão de resultado.
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        let details = rect_of(&node(&out, Role::Button, "Ver detalhes"));
        click_sized(&ctx, WINDOW, details.center(), &mut add);
        drop(add);
        settle(&mut m, &shared);
        assert_eq!(
            m.result.as_ref().map(|r| r["summary"]["id"].clone()),
            Some(json!("p-2")),
            "Ver detalhes reads the proposal of the ID"
        );
        assert!(m.receipt.is_none());
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(&out, Role::Label, "id: p-2");
        // Rejeitar…: a prévia leva o ID e o motivo.
        m.clear();
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = run_sized(&ctx, WINDOW, vec![], &mut add);
        assert_eq!(nodes(&out, Role::TextInput, "duplicada").len(), 1);
        let reject = rect_of(&node(&out, Role::Button, "Rejeitar…"));
        click_sized(&ctx, WINDOW, reject.center(), &mut add);
        drop(add);
        settle(&mut m, &shared);
        assert_eq!(
            m.receipt.as_ref().map(|r| &r.action.intent),
            Some(&Intent::Reject {
                id: "p-2".into(),
                reason: "duplicada".into()
            })
        );
        // Sem ID, as ações ficam desabilitadas.
        m.clear();
        m.identifier.clear();
        let out = run_sized(&ctx, WINDOW, vec![], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        for button in ["Ver detalhes", "Rejeitar…", "Aprovar…"] {
            assert!(node(&out, Role::Button, button).is_disabled(), "{button}");
        }
    }

    #[test]
    fn while_busy_the_project_and_the_tabs_are_locked_but_the_list_can_refresh() {
        for busy in [false, true] {
            let (mock, shared, mut m) = quiet();
            m.scope = Some(scope());
            m.busy = busy;
            let api = projects(&[scope(), other()]);
            let ctx = context(egui::Theme::Dark);
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            assert_eq!(
                node(&out, Role::ComboBox, "fixture / demo").is_disabled(),
                busy
            );
            assert_eq!(segment(&out, "Handoffs").is_disabled(), busy);
            let refresh = node(&out, Role::Button, "Atualizar");
            assert!(!refresh.is_disabled(), "busy {busy}: the list is its own");
            let until = Instant::now() + PATIENCE;
            while listed(&mock, "demo") == 0 {
                assert!(
                    Instant::now() < until,
                    "busy {busy}: the list was never asked"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            click_sized(&ctx, WINDOW, rect_of(&refresh).center(), &mut add);
            drop(add);
            settle_list(&mut m, &shared);
            assert_eq!(listed(&mock, "demo"), 2, "busy {busy}");
            assert_eq!(m.busy, busy);
        }
    }

    #[test]
    fn switching_connection_discards_preview_and_private_form_state() {
        let shared = Shared::unstarted(crate::model::Config::default());
        let mut ui = Management::new(shared.clone());
        ui.receipt = Some(Receipt {
            action: Action {
                scope: Scope {
                    workspace: "fixture".into(),
                    project: "demo".into(),
                },
                intent: Intent::Checkpoint {
                    message: "test".into(),
                },
            },
            generation: 0,
            at: std::time::Instant::now(),
        });
        ui.confirm = "GLOBAL".into();
        ui.reason = "private fixture".into();
        ui.identifier = "old-id".into();
        ui.result = Some(json!({"private":"fixture"}));
        ui.list.key = Some((scope(), "pending".into()));
        ui.list.value = Some(Ok(vec![proposal(1)]));
        ui.detail.key = Some((scope(), "old-id".into()));
        ui.detail.value = Some(Ok(proposal_detail(1)));
        ui.open = true;
        ui.reset();
        assert!(ui.receipt.is_none());
        assert!(ui.confirm.is_empty());
        assert!(ui.reason.is_empty());
        assert!(ui.identifier.is_empty());
        assert!(ui.result.is_none());
        assert!(
            ui.list.key.is_none() && ui.list.value.is_none(),
            "the list of the old connection goes too"
        );
        assert!(
            ui.detail.key.is_none() && ui.detail.value.is_none(),
            "and the detail"
        );
        assert!(!ui.open);
    }

    #[test]
    fn expired_preview_receipt_is_cleared_during_poll() {
        let shared = Shared::unstarted(crate::model::Config::default());
        let mut ui = Management::new(shared.clone());
        let receipt = |age: u64| Receipt {
            action: Action {
                scope: Scope {
                    workspace: "fixture".into(),
                    project: "demo".into(),
                },
                intent: Intent::Checkpoint {
                    message: "test".into(),
                },
            },
            generation: 0,
            at: std::time::Instant::now() - std::time::Duration::from_secs(age),
        };
        ui.receipt = Some(receipt(301));
        ui.result = Some(json!({"preview": true}));
        ui.error = Some("stale error".into());
        ui.confirm = "GLOBAL".into();
        ui.poll(&shared);
        assert!(
            ui.receipt.is_none(),
            "expired receipt must be cleared during poll so form fields unlock"
        );
        assert!(
            ui.result.is_none(),
            "expired preview result must be discarded, not kept on screen"
        );
        assert!(ui.error.is_none(), "expiry must clear stale errors");
        assert!(ui.confirm.is_empty());
        assert!(
            ui.notice.contains("expirou"),
            "expiry notice expected, got {:?}",
            ui.notice
        );

        ui.receipt = Some(receipt(299));
        ui.result = Some(json!({"preview": true}));
        ui.poll(&shared);
        assert!(
            ui.receipt.is_some() && ui.result.is_some(),
            "fresh preview must be kept"
        );
    }

    /// Um painel com a prévia de `intent` (pedida há `secs` segundos) na
    /// tela de Revisão · Propostas, num servidor sintético.
    fn previewing(intent: Intent, secs: u64) -> (Mock, Arc<Shared>, Management) {
        let mock = Mock::new(|_| Response::json(200, json!({"ok": true})));
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        m.receipt = Some(Receipt {
            action: Action {
                scope: scope(),
                intent,
            },
            generation: 0,
            at: Instant::now() - Duration::from_secs(secs),
        });
        m.result = Some(json!({"evidencia": "linha sintética da prévia"}));
        m.notice = "Consulta concluída; revise os dados abaixo.".into();
        (mock, shared, m)
    }
    const WINDOW: [f32; 2] = [1320., 860.];
    /// Dois frames: o primeiro da folha só mede (sizing pass).
    fn frames(ctx: &egui::Context, add: &mut impl FnMut(&mut Ui)) -> egui::FullOutput {
        run_sized(ctx, WINDOW, vec![], &mut *add);
        run_sized(ctx, WINDOW, vec![], &mut *add)
    }

    #[test]
    fn a_click_preview_opens_in_the_sheet_and_confirms_with_one_click() {
        let (mock, shared, mut m) = previewing(
            Intent::Checkpoint {
                message: "test".into(),
            },
            0,
        );
        let ctx = context(egui::Theme::Dark);
        let api = ApiSnapshot::default();
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        let out = frames(&ctx, &mut add);
        for text in [
            "Criar um checkpoint da wiki inteira?",
            "Projeto: fixture / demo",
            "Operação: Criar checkpoint global da wiki",
            "Alvo: Toda a wiki",
            "Prévia da alteração",
            "Prévia válida por 5:00",
        ] {
            node(&out, Role::Label, text);
        }
        // A prévia aparece uma vez, dentro da folha; o bloco antigo saiu.
        let preview = rect_of(&node(
            &out,
            Role::Label,
            "evidencia: linha sintética da prévia",
        ));
        let sheet = rect_of(&node(&out, Role::Label, "Prévia da alteração"));
        assert!(preview.top() > sheet.top(), "the preview is in the sheet");
        for gone in ["Resultado", "Revisar e confirmar", "Prévia / evidências"] {
            assert!(nodes(&out, Role::Label, gone).is_empty(), "{gone}");
        }
        let typing = out
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.role() == Role::Label
                    && [n.value(), n.label()]
                        .into_iter()
                        .flatten()
                        .any(|t| t.starts_with("Digite "))
            })
            .count();
        assert_eq!(typing, 0, "click level: nothing to type");
        let confirm = node(&out, Role::Button, "Criar checkpoint");
        assert!(!confirm.is_disabled());
        click_sized(&ctx, WINDOW, rect_of(&confirm).center(), &mut add);
        drop(add);
        assert!(m.busy, "the apply was submitted");
        assert!(m.receipt.is_none() && m.result.is_none());
        settle(&mut m, &shared);
        let requests = sent(&mock);
        assert_eq!(requests.len(), 1, "sent once");
        assert_eq!(
            (requests[0].method.as_str(), requests[0].path.as_str()),
            ("POST", "/admin/commit")
        );
    }

    /// A resposta da prévia de Aprovar/Rejeitar no formato do servidor
    /// (`maintenance.rs`): a proposta e a comparação.
    fn proposal_preview(operation: &str, before: &str, after: &str) -> Value {
        let target = "decisions/cache.md";
        json!({
            "proposal": {
                "summary": {"id": "p-1", "status": "pending", "operation": operation, "target_path": target},
                "body_markdown": after,
                "evidence_json": [{"page": "sessions/a.md", "quote": "expirou"}],
            },
            "comparison": {
                "proposal_id": "p-1",
                "diff": format!("--- before/{target}\n+++ after/{target}\n@@\n{before}\n--- proposed ---\n{after}\n"),
            },
        })
    }

    #[test]
    fn the_sheet_shows_target_effect_and_the_diff_or_falls_back_to_the_values() {
        let update = proposal_preview(
            "update",
            "# Cache\nTTL: 5 min\nfim",
            "# Cache\nTTL: 15 min\nfim",
        );
        let create = proposal_preview("create", "", "# Novo\n- item");
        // A comparação que não tem o formato esperado.
        let mut odd = update.clone();
        odd["comparison"]["diff"] = json!("before -> after");
        let approve = || Intent::Approve { id: "p-1".into() };
        // (intenção, resposta, fatos e linhas na folha, o que não aparece)
        let cases: [(Intent, Value, Vec<&str>, Vec<&str>); 5] = [
            (
                approve(),
                update.clone(),
                vec![
                    "Alvo: decisions/cache.md",
                    "Efeito: Grava uma nova versão da página; a atual fica no histórico",
                    "Reversível: Sim, restaurando a versão anterior em Manutenção e backup",
                    "  # Cache",
                    "- TTL: 5 min",
                    "+ TTL: 15 min",
                    "  fim",
                ],
                vec!["Alvo: fixture / demo", "diff: before -> after"],
            ),
            (
                approve(),
                create,
                vec![
                    "Alvo: decisions/cache.md",
                    "Efeito: Cria a página",
                    "Reversível: Não há versão anterior para restaurar",
                    "+ # Novo",
                    "+ - item",
                ],
                vec!["- item", "Alvo: fixture / demo"],
            ),
            (
                Intent::Reject {
                    id: "p-1".into(),
                    reason: "duplicada".into(),
                },
                update.clone(),
                vec![
                    "Alvo: decisions/cache.md",
                    "Efeito: Recusa a proposta; a página não muda",
                    "Motivo: duplicada",
                    "- TTL: 5 min",
                    "+ TTL: 15 min",
                ],
                vec!["Alvo: fixture / demo"],
            ),
            (
                approve(),
                odd,
                vec!["Alvo: fixture / demo", "diff: before -> after"],
                vec![
                    "Alvo: decisions/cache.md",
                    "+ TTL: 15 min",
                    "Efeito: Grava uma nova versão da página; a atual fica no histórico",
                ],
            ),
            (
                approve(),
                json!({"evidencia": "linha sintética da prévia"}),
                vec![
                    "Alvo: fixture / demo",
                    "evidencia: linha sintética da prévia",
                ],
                vec!["Efeito: Cria a página"],
            ),
        ];
        for (n, (intent, result, shown, absent)) in cases.into_iter().enumerate() {
            for (theme, p) in crate::widgets::tests::THEMES {
                let (_mock, shared, mut m) = previewing(intent.clone(), 0);
                let receipt = m.receipt.take().unwrap();
                m.hold_preview(receipt, result.clone());
                let ctx = context(theme);
                let api = ApiSnapshot::default();
                let out = frames(&ctx, &mut |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                for text in &shown {
                    assert_eq!(
                        nodes(&out, Role::Label, text).len(),
                        1,
                        "case {n} {theme:?}: {text}"
                    );
                }
                for text in &absent {
                    assert!(
                        nodes(&out, Role::Label, text).is_empty(),
                        "case {n} {theme:?}: {text}"
                    );
                }
                // Reversível só onde a tabela o mostra (Aprovar): Rejeitar
                // não tem a linha.
                let reversible = out
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .filter(|(_, l)| {
                        l.role() == Role::Label
                            && l.label()
                                .or(l.value())
                                .is_some_and(|t| t.starts_with("Reversível"))
                    })
                    .count();
                assert_eq!(
                    reversible,
                    usize::from(shown.iter().any(|t| t.starts_with("Reversível: "))),
                    "case {n} {theme:?}: Reversível"
                );
                // − em bad sobre bad_bg, + em acc sobre add_bg, iguais em text2.
                let painted = texts(&out);
                for line in &shown {
                    let (color, fill) = if line.starts_with("- ") {
                        (p.bad, Some(p.bad_bg))
                    } else if line.starts_with("+ ") {
                        (p.acc, Some(p.add_bg))
                    } else if line.starts_with("  ") {
                        (p.text2, None)
                    } else {
                        continue;
                    };
                    assert!(
                        painted.iter().any(|(t, c)| t == line && *c == color),
                        "case {n} {theme:?}: {line} in {color:?}"
                    );
                    if let Some(fill) = fill {
                        let row = rect_of(&node(&out, Role::Label, line));
                        assert!(
                            rects(&out)
                                .iter()
                                .any(|r| r.fill == fill && r.rect.contains(row.center())),
                            "case {n} {theme:?}: {line} on {fill:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_diff_is_computed_once_when_the_receipt_arrives_and_dropped_with_it() {
        let preview = proposal_preview(
            "update",
            "# Cache\nTTL: 5 min\nfim",
            "# Cache\nTTL: 15 min\nfim",
        );
        let mock = Mock::new(move |r| {
            if r.path.starts_with("/admin/pending-writes/p-1/diff?") {
                Response::json(200, preview["comparison"].clone())
            } else if r.path.starts_with("/admin/pending-writes/p-1?") {
                Response::json(200, preview["proposal"].clone())
            } else {
                Response::json(200, json!([]))
            }
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        let api = projects(&[scope()]);
        let ctx = context(egui::Theme::Dark);
        let computed = || PROPOSED.with(std::cell::Cell::get);
        PROPOSED.with(|n| n.set(0));
        // A prévia chega pelo `poll`, como na tela.
        let arm = |m: &mut Management, intent: Intent| {
            m.preview(&shared, &scope(), intent);
            settle(m, &shared);
            assert!(m.receipt.is_some(), "precondition: the preview arrived");
        };
        let approve = || Intent::Approve { id: "p-1".into() };
        arm(&mut m, approve());
        assert_eq!(computed(), 1, "computed when the receipt arrived");
        let mut out = None;
        for _ in 0..10 {
            out = Some(run_sized(&ctx, WINDOW, vec![], |ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            }));
        }
        let out = out.unwrap();
        node(&out, Role::Label, "+ TTL: 15 min");
        node(
            &out,
            Role::Label,
            "Efeito: Grava uma nova versão da página; a atual fica no histórico",
        );
        assert_eq!(computed(), 1, "10 frames with the sheet open, one diff");
        // Um recibo novo recalcula: o efeito passa a ser o da rejeição.
        arm(
            &mut m,
            Intent::Reject {
                id: "p-1".into(),
                reason: String::new(),
            },
        );
        assert_eq!(computed(), 2, "a new receipt computes its own diff");
        let out = frames(&ctx, &mut |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        node(
            &out,
            Role::Label,
            "Efeito: Recusa a proposta; a página não muda",
        );
        node(&out, Role::Label, "+ TTL: 15 min");
        assert_eq!(computed(), 2);
        // Todo caminho que descarta o recibo descarta o diff.
        for way in ["clear", "discard", "reset", "expiry"] {
            m.scope = Some(scope());
            arm(&mut m, approve());
            assert!(m.proposed.is_some(), "{way}: precondition");
            match way {
                "clear" => m.clear(),
                "discard" => m.discard(),
                "reset" => m.reset(),
                _ => {
                    if let Some(r) = m.receipt.as_mut() {
                        r.at = Instant::now() - PREVIEW_TTL - Duration::from_secs(1);
                    }
                }
            }
            m.poll(&shared);
            assert!(m.receipt.is_none(), "{way}: precondition");
            assert!(m.proposed.is_none(), "{way}: the diff outlived its receipt");
        }
        // Cancelar na folha (Esc) também.
        arm(&mut m, approve());
        let esc = egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        frames(&ctx, &mut |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        run_sized(&ctx, WINDOW, vec![esc], |ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        });
        assert!(m.receipt.is_none(), "cancel: precondition");
        assert!(
            m.proposed.is_none(),
            "cancel: the diff outlived its receipt"
        );
        // Uma leitura não traz recibo nem diff.
        m.begin(&shared, Work::Read(Read::Checkpoints));
        settle(&mut m, &shared);
        assert!(m.result.is_some() && m.receipt.is_none() && m.proposed.is_none());
    }

    #[test]
    fn the_sheet_shows_the_reason_line_only_when_a_reason_was_given() {
        // (motivo, a linha na folha)
        for (reason, line) in [
            ("", None),
            ("   ", None),
            ("duplicada", Some("Motivo: duplicada")),
        ] {
            let cases = [
                (
                    Intent::Reject {
                        id: "p-1".into(),
                        reason: reason.into(),
                    },
                    0,
                    Section::Review,
                ),
                (
                    Intent::Feedback {
                        path: "decisions/cache.md".into(),
                        signal: "stale".into(),
                        reason: reason.into(),
                    },
                    2,
                    Section::Maintenance,
                ),
            ];
            for (intent, tab, section) in cases {
                let operation = Action {
                    scope: scope(),
                    intent: intent.clone(),
                }
                .label();
                let (_mock, shared, mut m) = previewing(intent, 0);
                m.tab = tab;
                let ctx = context(egui::Theme::Light);
                let api = ApiSnapshot::default();
                let out = frames(&ctx, &mut |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, section);
                });
                let case = format!("{operation} {reason:?}");
                // A folha está aberta, com a grade.
                node(&out, Role::Label, &format!("Operação: {operation}"));
                let shown: Vec<String> = out
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .filter(|(_, n)| n.role() == Role::Label)
                    .filter_map(|(_, n)| n.label().or(n.value()))
                    .filter(|t| t.starts_with("Motivo:"))
                    .map(str::to_owned)
                    .collect();
                assert_eq!(
                    shown,
                    line.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                    "{case}: the reason line"
                );
            }
        }
    }

    #[test]
    fn changes_keep_three_lines_around_each_change_and_summarize_the_rest() {
        let owned = |lines: &[(Mark, &str)]| -> Vec<(Mark, String)> {
            lines.iter().map(|&(m, t)| (m, t.to_owned())).collect()
        };
        // Página nova: tudo acrescentado.
        assert_eq!(
            changes("", "a\n- b"),
            owned(&[(Mark::Added, "a"), (Mark::Added, "- b")])
        );
        // Igual: só o resumo.
        assert_eq!(
            changes("a\nb", "a\nb"),
            owned(&[(Mark::Skipped, "⋯ 2 linhas iguais")])
        );
        // Uma linha trocada no meio de 11: 3 de contexto de cada lado.
        let before: Vec<String> = (1..=11).map(|i| format!("linha {i}")).collect();
        let mut after = before.clone();
        after[5] = "linha seis".into();
        assert_eq!(
            changes(&before.join("\n"), &after.join("\n")),
            owned(&[
                (Mark::Skipped, "⋯ 2 linhas iguais"),
                (Mark::Same, "linha 3"),
                (Mark::Same, "linha 4"),
                (Mark::Same, "linha 5"),
                (Mark::Removed, "linha 6"),
                (Mark::Added, "linha seis"),
                (Mark::Same, "linha 7"),
                (Mark::Same, "linha 8"),
                (Mark::Same, "linha 9"),
                (Mark::Skipped, "⋯ 2 linhas iguais"),
            ])
        );
        // Uma linha igual só, antes do contexto.
        assert_eq!(
            changes("a\nb\nc\nd\ne", "a\nb\nc\nd\nE")[0],
            (Mark::Skipped, "⋯ 1 linha igual".into())
        );
        // Inserção e remoção sem troca.
        assert_eq!(
            changes("x\ny", "x\nnovo\ny"),
            owned(&[(Mark::Same, "x"), (Mark::Added, "novo"), (Mark::Same, "y")])
        );
        assert_eq!(
            changes("x\nvelho\ny", "x\ny"),
            owned(&[
                (Mark::Same, "x"),
                (Mark::Removed, "velho"),
                (Mark::Same, "y")
            ])
        );
        // Grande demais para o LCS: o meio sai inteiro removido e
        // acrescentado, mesmo com linhas iguais nele (que o LCS acharia).
        let lines = |tag: &str| -> Vec<String> {
            (0..300)
                .flat_map(|i| [format!("{tag}{i}"), "igual".to_owned()])
                .collect()
        };
        let (before, after) = (lines("a"), lines("b"));
        // O último "igual" é o sufixo comum; o meio tem 599 linhas de cada lado.
        let n = before.len() - 1;
        assert!(n * n > DIFF_CELLS, "precondition: above the limit");
        let got = changes(&before.join("\n"), &after.join("\n"));
        assert_eq!(got.len(), 2 * n + 1);
        assert!(got[..n].iter().all(|(m, _)| *m == Mark::Removed));
        assert!(got[n..2 * n].iter().all(|(m, _)| *m == Mark::Added));
        assert_eq!(got[2 * n], (Mark::Same, "igual".into()));
        assert_eq!((got[0].1.as_str(), got[n].1.as_str()), ("a0", "b0"));
        // Abaixo do limite, as iguais do meio aparecem.
        let (before, after) = (&before[..200], &after[..200]);
        let got = changes(&before.join("\n"), &after.join("\n"));
        assert!(got.iter().any(|(m, t)| *m == Mark::Same && t == "igual"));
    }

    #[test]
    fn a_typed_preview_waits_for_the_exact_project_label() {
        for (typed, enabled) in [
            ("", false),
            ("fixture", false),
            ("fixture / dem", false),
            ("fixture / demo ", false),
            ("CONFIRMAR fixture / demo", false),
            ("SESSAO fixture / demo", false),
            ("GLOBAL", false),
            ("fixture / demo", true),
        ] {
            let (mock, shared, mut m) = previewing(Intent::CancelHandoff { id: "h-1".into() }, 0);
            m.confirm = typed.into();
            let ctx = context(egui::Theme::Light);
            let api = ApiSnapshot::default();
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = frames(&ctx, &mut add);
            node(&out, Role::Label, "Digite fixture / demo para confirmar");
            node(
                &out,
                Role::Label,
                "Cancelar o handoff “h-1” em “fixture / demo”?",
            );
            let confirm = node(&out, Role::Button, "Cancelar handoff");
            assert_eq!(confirm.is_disabled(), !enabled, "{typed:?}");
            click_sized(&ctx, WINDOW, rect_of(&confirm).center(), &mut add);
            drop(add);
            assert_eq!(m.busy, enabled, "{typed:?}");
            assert_eq!(m.receipt.is_some(), !enabled, "{typed:?}");
            settle(&mut m, &shared);
            assert_eq!(sent(&mock).len(), usize::from(enabled), "{typed:?}");
        }
    }

    #[test]
    fn cancel_and_escape_close_the_sheet_and_clear_the_receipt_and_the_text() {
        for way in ["button", "escape"] {
            let (mock, shared, mut m) = previewing(Intent::ExpireHandoffs { days: 7 }, 0);
            m.confirm = "fixture / de".into();
            m.identifier = "draft-id".into();
            let ctx = context(egui::Theme::Dark);
            let api = ApiSnapshot::default();
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = frames(&ctx, &mut add);
            match way {
                "button" => {
                    let at = rect_of(&node(&out, Role::Button, "Cancelar")).center();
                    click_sized(&ctx, WINDOW, at, &mut add);
                }
                _ => {
                    let key = |pressed| egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Default::default(),
                    };
                    run_sized(&ctx, WINDOW, vec![key(true), key(false)], &mut add);
                }
            }
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            drop(add);
            assert!(m.receipt.is_none() && m.result.is_none(), "{way}");
            assert_eq!((m.confirm.as_str(), m.notice.as_str()), ("", ""), "{way}");
            assert_eq!(m.identifier, "draft-id", "{way}: the form keeps its fields");
            assert!(!m.holds(), "{way}");
            assert!(nodes(&out, Role::Button, "Expirar").is_empty(), "{way}");
            // Nada além da leitura do detalhe do ID no campo.
            let asked: Vec<(String, String)> = sent(&mock)
                .into_iter()
                .map(|r| (r.method, r.path))
                .collect();
            assert!(
                asked.iter().all(|(method, path)| method == "GET"
                    && path.starts_with("/admin/pending-writes/draft-id?")),
                "{way}: nothing was sent, {asked:?}"
            );
        }
    }

    #[test]
    fn the_veil_keeps_the_tabs_under_the_sheet_from_discarding_the_preview() {
        for open in [true, false] {
            let (_mock, shared, mut m) = previewing(Intent::Approve { id: "p-1".into() }, 0);
            if !open {
                m.receipt = None;
            }
            let ctx = context(egui::Theme::Dark);
            let api = ApiSnapshot::default();
            let mut add = |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            };
            let out = frames(&ctx, &mut add);
            let tab = rect_of(&segment(&out, "Handoffs"));
            if open {
                let title = rect_of(&node(&out, Role::Label, "Aprovar proposta?"));
                assert!(
                    tab.right() < title.left(),
                    "precondition: the tab is outside the sheet"
                );
            }
            click_sized(&ctx, WINDOW, tab.center(), &mut add);
            drop(add);
            assert_eq!(m.tab, if open { 0 } else { 1 }, "sheet open: {open}");
            assert_eq!(m.receipt.is_some(), open, "sheet open: {open}");
        }
    }

    #[test]
    fn the_keyboard_never_reaches_what_would_discard_the_preview_under_the_sheet() {
        let expire = || Intent::ExpireHandoffs { days: 7 };
        // (aba, prévia, os controles a alcançar com Tab e apertar com Enter)
        let cases: [(u8, Intent, &[&str]); 5] = [
            (0, Intent::Approve { id: "p-1".into() }, &["Handoffs"]),
            (1, expire(), &["Propostas"]),
            (1, expire(), &["Carregar histórico"]),
            (1, expire(), &["Expirar antigos…"]),
            // O seletor abre com Enter; o Tab entra nas opções.
            (0, expire(), &["fixture / demo", "fixture / other"]),
        ];
        for (tab, intent, controls) in cases {
            for open in [true, false] {
                let (_mock, shared, mut m) = previewing(intent.clone(), 0);
                m.tab = tab;
                if !open {
                    m.receipt = None;
                }
                let ctx = context(egui::Theme::Dark);
                let api = projects(&[scope(), other()]);
                let mut add = |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                };
                frames(&ctx, &mut add);
                // O segmentado fala "rótulo contador".
                let hit = |control: &str, l: &str| {
                    l == control || l.strip_prefix(control).is_some_and(|r| r.starts_with(' '))
                };
                let mut seen = Vec::new();
                for control in controls {
                    seen.extend(tab_and_enter(
                        &ctx,
                        WINDOW,
                        &mut add,
                        |l| hit(control, l),
                        40,
                    ));
                }
                drop(add);
                let case = format!("tab {tab} {controls:?}, sheet open: {open}, focus: {seen:?}");
                assert_eq!(
                    m.receipt.is_some(),
                    open,
                    "{case}: Tab + Enter under the sheet discarded the preview"
                );
                // Sem a folha, o Enter faz o que o clique faria.
                let acted = m.tab != tab || m.busy || m.scope != Some(scope());
                assert_eq!(acted, !open, "{case}");
                for control in controls {
                    assert_eq!(
                        seen.iter().any(|l| hit(control, l)),
                        !open,
                        "{case}: {control}"
                    );
                }
                if open {
                    // O teclado andou: só não chega ao que está sob o véu.
                    assert!(seen.iter().any(|l| l == "Cancelar"), "{case}");
                }
            }
        }
    }

    #[test]
    fn the_keyboard_never_rereads_or_swaps_the_list_under_the_sheet() {
        // (os controles a alcançar com Tab e apertar com Enter, o estado sem
        // a folha). O seletor abre com Enter; o Tab entra nas opções.
        let cases: [(&[&str], &str); 2] = [
            (&["Atualizar"], "pending"),
            (&["Estado: pendentes", "Estado: aprovadas"], "approved"),
        ];
        for (controls, status) in cases {
            for open in [true, false] {
                let (mock, shared, mut m) = previewing(Intent::Approve { id: "p-1".into() }, 0);
                if !open {
                    m.receipt = None;
                }
                let ctx = context(egui::Theme::Dark);
                let api = projects(&[scope(), other()]);
                frames(&ctx, &mut |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                });
                // A lista da abertura respondeu: um pedido novo é do teclado.
                settle_list(&mut m, &shared);
                let ticket = m.list.ticket;
                let mut add = |ui: &mut Ui| {
                    page(&mut m, ui, &shared, &api, Section::Review);
                };
                let mut seen = Vec::new();
                for control in controls {
                    seen.extend(tab_and_enter(&ctx, WINDOW, &mut add, |l| l == *control, 40));
                }
                drop(add);
                let case = format!("{controls:?}, sheet open: {open}, focus: {seen:?}");
                assert_eq!(m.receipt.is_some(), open, "{case}");
                // Sem a folha, o Enter faz o que o clique faria: Atualizar
                // relê a lista e Estado a troca. Com ela, nada muda atrás.
                assert_eq!(m.list.ticket != ticket, !open, "{case}: the list asked");
                assert_eq!(m.status, if open { "pending" } else { status }, "{case}");
                for control in controls {
                    assert_eq!(
                        seen.iter().any(|l| l == control),
                        !open,
                        "{case}: {control}"
                    );
                }
                if open {
                    // O teclado andou: só não chega ao que está sob o véu.
                    assert!(seen.iter().any(|l| l == "Cancelar"), "{case}");
                } else {
                    settle_list(&mut m, &shared);
                }
                assert_eq!(listed(&mock, "demo"), 1 + usize::from(!open), "{case}");
            }
        }
    }

    #[test]
    fn a_preview_opens_the_sheet_without_a_notice_and_a_read_still_points_below() {
        let below = "Consulta concluída; revise os dados abaixo.";
        for preview in [true, false] {
            let mock = Mock::new(|_| Response::json(200, json!([])));
            let shared = Shared::unstarted(mock.config());
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            m.tab = 3;
            // Um aviso anterior qualquer, para o novo ter de substituí-lo.
            m.notice = "aviso anterior".into();
            let work = if preview {
                Work::Preview(Action {
                    scope: scope(),
                    intent: Intent::Checkpoint {
                        message: "test".into(),
                    },
                })
            } else {
                Work::Read(Read::Checkpoints)
            };
            m.begin(&shared, work);
            m.notice = "aviso anterior".into();
            settle(&mut m, &shared);
            assert_eq!(m.receipt.is_some(), preview, "preview {preview}");
            assert!(m.result.is_some(), "preview {preview}");
            let want = if preview { "" } else { below };
            assert_eq!(m.notice, want, "preview {preview}");
            let ctx = context(egui::Theme::Dark);
            let api = ApiSnapshot::default();
            let out = frames(&ctx, &mut |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Maintenance);
            });
            assert_eq!(
                nodes(&out, Role::Label, below).len(),
                usize::from(!preview),
                "preview {preview}"
            );
            assert_eq!(
                nodes(&out, Role::Button, "Criar checkpoint").len(),
                usize::from(preview),
                "preview {preview}: the sheet"
            );
        }
    }

    #[test]
    fn a_pasted_confirmation_in_review_stops_at_512_characters() {
        let (_mock, shared, mut m) = previewing(Intent::ExpireHandoffs { days: 7 }, 0);
        let ctx = context(egui::Theme::Light);
        let api = ApiSnapshot::default();
        let mut add = |ui: &mut Ui| {
            page(&mut m, ui, &shared, &api, Section::Review);
        };
        frames(&ctx, &mut add);
        run_sized(
            &ctx,
            WINDOW,
            vec![egui::Event::Paste("x".repeat(600))],
            &mut add,
        );
        drop(add);
        assert_eq!(m.confirm, "x".repeat(512));
    }

    #[test]
    fn the_countdown_follows_the_receipt_and_an_expired_one_closes_the_sheet() {
        let expired = "A prévia expirou após 5 minutos. Prepare a operação novamente.";
        for (age, countdown) in [(0, Some("5:00")), (299, Some("0:01")), (301, None)] {
            let (_mock, shared, mut m) = previewing(
                Intent::Maintain {
                    task: Task::Sweep,
                    reembed: false,
                },
                age,
            );
            let ctx = context(egui::Theme::Light);
            let api = ApiSnapshot::default();
            let out = frames(&ctx, &mut |ui: &mut Ui| {
                page(&mut m, ui, &shared, &api, Section::Review);
            });
            let shown = countdown.map(|c| format!("Prévia válida por {c}"));
            for text in ["Prévia válida por 5:00", "Prévia válida por 0:01"] {
                assert_eq!(
                    nodes(&out, Role::Label, text).len(),
                    usize::from(shown.as_deref() == Some(text)),
                    "{age} s: {text}"
                );
            }
            assert_eq!(
                nodes(&out, Role::Button, "Aplicar retenção").len(),
                usize::from(countdown.is_some()),
                "{age} s"
            );
            assert_eq!(
                nodes(&out, Role::Label, expired).len(),
                usize::from(countdown.is_none()),
                "{age} s"
            );
            assert_eq!(m.receipt.is_some(), countdown.is_some(), "{age} s");
            assert_eq!(m.expired, countdown.is_none(), "{age} s");
        }
    }
}
