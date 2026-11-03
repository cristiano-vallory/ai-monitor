//! `color-scheme` do portal freedesktop (org.freedesktop.appearance), lido e
//! acompanhado numa thread de fundo: a ativação do portal pode levar segundos
//! e a UI nunca espera por ela. A UI só lê o último valor.

use std::sync::atomic::{AtomicU64, Ordering};
use zbus::zvariant::{OwnedValue, Value};

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

/// Fora do domínio de u32: distingue "sem resposta" de qualquer valor do portal.
const UNKNOWN: u64 = u64::MAX;
static SCHEME: AtomicU64 = AtomicU64::new(UNKNOWN);

pub fn current() -> Option<u32> {
    decode(SCHEME.load(Ordering::Acquire))
}
fn store(scheme: Option<u32>) {
    SCHEME.store(encode(scheme), Ordering::Release);
}
fn encode(scheme: Option<u32>) -> u64 {
    scheme.map_or(UNKNOWN, u64::from)
}
fn decode(raw: u64) -> Option<u32> {
    u32::try_from(raw).ok()
}

/// Sem barramento de sessão ou sem portal, o valor fica `None` (tema escuro).
pub fn start() {
    let spawned = std::thread::Builder::new()
        .name("portal-appearance".into())
        .spawn(|| {
            if let Err(e) =
                zbus::blocking::Connection::session().and_then(|conn| watch(&conn, store))
            {
                eprintln!("AI Monitor: portal de aparência indisponível ({e}); tema escuro.");
            }
        });
    if let Err(e) = spawned {
        eprintln!("AI Monitor: não foi possível acompanhar o tema do sistema: {e}");
    }
}

/// Lê o valor atual e repassa cada mudança a `on_value` até a conexão fechar.
fn watch(
    conn: &zbus::blocking::Connection,
    mut on_value: impl FnMut(Option<u32>),
) -> zbus::Result<()> {
    let proxy = zbus::blocking::Proxy::new(conn, DESTINATION, PATH, INTERFACE)?;
    // Assina antes de ler: uma troca entre a leitura e a assinatura não se perde.
    let signals = proxy.receive_signal("SettingChanged")?;
    on_value(read(|method| proxy.call(method, &(NAMESPACE, KEY))));
    for message in signals {
        if let Ok((namespace, key, value)) =
            message.body().deserialize::<(String, String, OwnedValue)>()
            && let Some(scheme) = changed(&namespace, &key, &value)
        {
            on_value(scheme);
        }
    }
    Ok(())
}

/// `ReadOne` (Settings v2) primeiro; se falhar, `Read` (v1).
fn read(mut call: impl FnMut(&str) -> zbus::Result<OwnedValue>) -> Option<u32> {
    let reply = call("ReadOne").or_else(|_| call("Read")).ok()?;
    scheme(&reply)
}

/// `Some(valor)` somente para org.freedesktop.appearance / color-scheme;
/// `None` = outra configuração, a ser ignorada.
fn changed(namespace: &str, key: &str, value: &Value<'_>) -> Option<Option<u32>> {
    (namespace == NAMESPACE && key == KEY).then(|| scheme(value))
}

/// `ReadOne` devolve `u`; `Read` embrulha num variant a mais: `v(u)`.
fn scheme(value: &Value<'_>) -> Option<u32> {
    match value {
        Value::U32(n) => Some(*n),
        Value::Value(inner) => match **inner {
            Value::U32(n) => Some(n),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{LE, serialized::Context, to_bytes};

    fn owned(value: Value<'_>) -> OwnedValue {
        value.try_to_owned().unwrap()
    }
    fn wrapped(n: u32) -> Value<'static> {
        Value::Value(Box::new(Value::U32(n)))
    }

    #[test]
    fn scheme_accepts_u_and_v_of_u_only() {
        for n in [0, 1, 2, 3, u32::MAX] {
            assert_eq!(scheme(&Value::U32(n)), Some(n), "u {n}");
            assert_eq!(scheme(&wrapped(n)), Some(n), "v(u) {n}");
        }
        for wrong in [
            Value::Str("dark".into()),
            Value::I32(1),
            Value::U8(1),
            Value::U64(1),
            Value::Bool(true),
            Value::Value(Box::new(Value::Str("1".into()))),
            Value::Value(Box::new(wrapped(1))),
        ] {
            assert_eq!(scheme(&wrong), None, "{wrong:?}");
        }
    }

    #[test]
    fn wire_bodies_of_read_one_and_read_decode_to_u_and_v_of_u() {
        // Corpo `v` de ReadOne (variant com `u`) e de Read (variant com `v(u)`),
        // codificados como na mensagem D-Bus e lidos como OwnedValue, que é o
        // tipo pedido em `proxy.call`.
        let ctxt = Context::new_dbus(LE, 0);
        let read_one = to_bytes(ctxt, &Value::U32(1)).unwrap();
        let (v, _): (OwnedValue, _) = read_one.deserialize().unwrap();
        assert!(matches!(*v, Value::U32(1)), "{v:?}");
        assert_eq!(scheme(&v), Some(1));
        let read = to_bytes(ctxt, &wrapped(2)).unwrap();
        let (v, _): (OwnedValue, _) = read.deserialize().unwrap();
        assert!(matches!(*v, Value::Value(_)), "{v:?}");
        assert_eq!(scheme(&v), Some(2));
    }

    #[test]
    fn read_prefers_read_one_and_falls_back_to_read() {
        type Answer = fn(&str) -> zbus::Result<OwnedValue>;
        let cases: [(&str, Answer, Option<u32>, &[&str]); 5] = [
            (
                "ReadOne answers",
                |_| Ok(owned(Value::U32(2))),
                Some(2),
                &["ReadOne"],
            ),
            (
                "ReadOne missing, Read answers v(u)",
                |m| match m {
                    "ReadOne" => Err(zbus::Error::Failure("UnknownMethod".into())),
                    _ => Ok(owned(wrapped(1))),
                },
                Some(1),
                &["ReadOne", "Read"],
            ),
            (
                "both fail",
                |_| Err(zbus::Error::Failure("synthetic".into())),
                None,
                &["ReadOne", "Read"],
            ),
            (
                "ReadOne answers a wrong type: no fallback",
                |_| Ok(owned(Value::Str("dark".into()))),
                None,
                &["ReadOne"],
            ),
            (
                "ReadOne answers out of range",
                |_| Ok(owned(Value::U32(7))),
                Some(7),
                &["ReadOne"],
            ),
        ];
        for (case, answer, want, calls) in cases {
            let mut seen = Vec::new();
            let got = read(|m| {
                seen.push(m.to_owned());
                answer(m)
            });
            assert_eq!(got, want, "{case}");
            assert_eq!(seen, calls, "{case}");
        }
    }

    #[test]
    fn setting_changed_is_filtered_by_namespace_and_key() {
        let one = Value::U32(1);
        assert_eq!(changed(NAMESPACE, KEY, &one), Some(Some(1)));
        assert_eq!(changed(NAMESPACE, KEY, &Value::U32(2)), Some(Some(2)));
        assert_eq!(
            changed(NAMESPACE, KEY, &Value::Str("dark".into())),
            Some(None),
            "the right key with a wrong type resets to unknown"
        );
        for (namespace, key) in [
            ("org.gnome.desktop.interface", KEY),
            (NAMESPACE, "accent-color"),
            (NAMESPACE, "contrast"),
            ("", ""),
        ] {
            assert_eq!(changed(namespace, key, &one), None, "{namespace} / {key}");
        }
    }

    #[test]
    fn stored_scheme_distinguishes_unknown_from_every_u32() {
        for value in [Some(0), Some(1), Some(2), Some(u32::MAX), None] {
            assert_eq!(decode(encode(value)), value, "{value:?}");
        }
        assert_eq!(decode(UNKNOWN), None);
    }

    /// Resposta programável do portal falso.
    #[derive(Clone, Copy)]
    enum Reply {
        Plain(u32),
        Wrapped(u32),
        Missing,
        Fail,
    }
    struct FakeState {
        read_one: Reply,
        read: Reply,
        /// Mudança emitida enquanto o ReadOne está em voo (antes da resposta).
        race: Option<u32>,
        calls: Vec<(String, String, String)>,
    }
    struct FakeSettings(std::sync::Arc<std::sync::Mutex<FakeState>>);
    impl FakeSettings {
        fn answer(&self, method: &str, namespace: String, key: String) -> (Reply, Option<u32>) {
            let mut state = self.0.lock().unwrap();
            state.calls.push((method.into(), namespace, key));
            let reply = if method == "ReadOne" {
                state.read_one
            } else {
                state.read
            };
            (reply, state.race.filter(|_| method == "ReadOne"))
        }
    }
    fn reply(r: Reply) -> zbus::fdo::Result<OwnedValue> {
        match r {
            Reply::Plain(n) => Ok(owned(Value::U32(n))),
            Reply::Wrapped(n) => Ok(owned(wrapped(n))),
            Reply::Missing => Err(zbus::fdo::Error::UnknownMethod("synthetic".into())),
            Reply::Fail => Err(zbus::fdo::Error::Failed("synthetic".into())),
        }
    }
    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl FakeSettings {
        async fn read_one(
            &self,
            #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
            namespace: String,
            key: String,
        ) -> zbus::fdo::Result<OwnedValue> {
            let (answer, race) = self.answer("ReadOne", namespace, key);
            if let Some(n) = race {
                Self::setting_changed(&emitter, NAMESPACE, KEY, Value::U32(n)).await?;
            }
            reply(answer)
        }
        async fn read(&self, namespace: String, key: String) -> zbus::fdo::Result<OwnedValue> {
            reply(self.answer("Read", namespace, key).0)
        }
        #[zbus(signal)]
        async fn setting_changed(
            emitter: &zbus::object_server::SignalEmitter<'_>,
            namespace: &str,
            key: &str,
            value: Value<'_>,
        ) -> zbus::Result<()>;
    }

    /// Barramento real (`dbus-run-session -- cargo test -- --ignored portal_`):
    /// o portal é falso, o cliente é o `watch()` de produção.
    #[test]
    #[ignore = "precisa de um dbus-daemon: dbus-run-session -- cargo test -- --ignored portal_"]
    fn portal_watch_on_session_bus() {
        use std::{
            sync::{Arc, Mutex, mpsc},
            time::Duration,
        };
        const WAIT: Duration = Duration::from_secs(10);
        let state = Arc::new(Mutex::new(FakeState {
            read_one: Reply::Fail,
            read: Reply::Fail,
            race: None,
            calls: Vec::new(),
        }));
        let server = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.serve_at(PATH, FakeSettings(state.clone())))
            .and_then(|b| b.name(DESTINATION))
            .and_then(|b| b.build())
            .expect("session bus: run under dbus-run-session");
        let program = |read_one, read, race| {
            let mut s = state.lock().unwrap();
            s.read_one = read_one;
            s.read = read;
            s.race = race;
            s.calls.clear();
        };
        let calls = || -> Vec<String> {
            let s = state.lock().unwrap();
            for (_, namespace, key) in &s.calls {
                assert_eq!((namespace.as_str(), key.as_str()), (NAMESPACE, KEY));
            }
            s.calls.iter().map(|(m, _, _)| m.clone()).collect()
        };
        // Cada caso abre o seu cliente; threads de casos anteriores seguem
        // vivas e só escrevem em canais já descartados.
        let watcher = || {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let conn = zbus::blocking::Connection::session().expect("client connection");
                let result = watch(&conn, |value| {
                    let _ = tx.send(Ok(value));
                });
                let _ = tx.send(Err(format!("watch returned: {result:?}")));
            });
            rx
        };
        let next = |rx: &mpsc::Receiver<Result<Option<u32>, String>>, what: &str| match rx
            .recv_timeout(WAIT)
        {
            Ok(Ok(value)) => value,
            Ok(Err(e)) => panic!("{what}: {e}"),
            Err(e) => panic!("{what}: no value within {WAIT:?} ({e})"),
        };
        let emit = |namespace: &str, key: &str, value: Value<'_>| {
            server
                .emit_signal(
                    None::<zbus::names::BusName<'_>>,
                    PATH,
                    INTERFACE,
                    "SettingChanged",
                    &(namespace, key, value),
                )
                .unwrap();
        };

        program(Reply::Plain(2), Reply::Fail, None);
        assert_eq!(next(&watcher(), "ReadOne answers"), Some(2));
        assert_eq!(calls(), ["ReadOne"]);

        program(Reply::Missing, Reply::Wrapped(1), None);
        assert_eq!(next(&watcher(), "ReadOne missing"), Some(1));
        assert_eq!(calls(), ["ReadOne", "Read"]);

        program(Reply::Fail, Reply::Fail, None);
        assert_eq!(next(&watcher(), "both fail"), None);
        assert_eq!(calls(), ["ReadOne", "Read"]);

        // Mudança durante o ReadOne: o sinal sai antes da resposta. Só não se
        // perde porque watch() assina antes de ler.
        program(Reply::Plain(2), Reply::Fail, Some(1));
        let rx = watcher();
        assert_eq!(next(&rx, "read with a change in flight"), Some(2));
        assert_eq!(
            next(&rx, "change emitted while ReadOne was in flight"),
            Some(1),
            "watch() must subscribe before reading"
        );

        // Sinais: o valor inicial chega depois da assinatura (provado acima),
        // então tudo o que for emitido a partir daqui chega a este cliente, em
        // ordem.
        program(Reply::Plain(0), Reply::Fail, None);
        let rx = watcher();
        assert_eq!(next(&rx, "initial value"), Some(0));
        emit("org.gnome.desktop.interface", KEY, Value::U32(1));
        emit(NAMESPACE, "accent-color", Value::U32(1));
        emit(NAMESPACE, KEY, Value::U32(2));
        assert_eq!(
            next(&rx, "right key after two ignored ones"),
            Some(2),
            "a SettingChanged of another namespace or key must be ignored"
        );
        emit(NAMESPACE, KEY, Value::Str("dark".into()));
        assert_eq!(next(&rx, "right key, wrong type"), None);
        emit(NAMESPACE, KEY, Value::U32(1));
        assert_eq!(next(&rx, "right key again"), Some(1));
    }
}
