//! Commands arriving at the app while a window may or may not exist.
//! The decision is pure so the whole domain is unit-testable.

/// `Open` carries a `menu::Destination` code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Open(u8),
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    OpenWindow(u8),
    FocusAndNavigate(u8),
    Exit,
}

pub fn decide(window_open: bool, cmd: Command) -> Action {
    match cmd {
        Command::Quit => Action::Exit,
        Command::Open(nav) => {
            // Outside the destinations: open without navigating.
            let nav = crate::menu::Destination::from_code(nav).map_or(0, |d| d.code());
            if window_open {
                Action::FocusAndNavigate(nav)
            } else {
                Action::OpenWindow(nav)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceDecision {
    Owner,
    Forward,
    Standalone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameReply {
    PrimaryOwner,
    AlreadyOwner,
    Exists,
    InQueue,
}

/// Maps the RequestName reply onto the single-instance decision. `Err(())`
/// stands for any D-Bus-level failure (e.g. sandbox policy), which must
/// never be mistaken for "another instance is running".
pub fn decide_instance(reply: Result<NameReply, ()>) -> InstanceDecision {
    match reply {
        Ok(NameReply::PrimaryOwner) | Ok(NameReply::AlreadyOwner) => InstanceDecision::Owner,
        Ok(NameReply::Exists) | Ok(NameReply::InQueue) => InstanceDecision::Forward,
        Err(()) => InstanceDecision::Standalone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_covers_every_command_and_window_state() {
        assert_eq!(decide(false, Command::Quit), Action::Exit);
        assert_eq!(decide(true, Command::Quit), Action::Exit);
        // 0 = panel, 1 = Saúde, 2 = Revisão, 3 = Sessões, 4 = Ajustes; the
        // rest of the u8 domain opens without navigating.
        for nav in 0..=u8::MAX {
            let want = if nav <= 4 { nav } else { 0 };
            assert_eq!(
                decide(false, Command::Open(nav)),
                Action::OpenWindow(want),
                "nav={nav}"
            );
            assert_eq!(
                decide(true, Command::Open(nav)),
                Action::FocusAndNavigate(want),
                "nav={nav}"
            );
        }
    }

    #[test]
    fn decide_instance_covers_every_request_name_outcome() {
        for reply in [NameReply::PrimaryOwner, NameReply::AlreadyOwner] {
            assert_eq!(
                decide_instance(Ok(reply)),
                InstanceDecision::Owner,
                "{reply:?}"
            );
        }
        for reply in [NameReply::Exists, NameReply::InQueue] {
            assert_eq!(
                decide_instance(Ok(reply)),
                InstanceDecision::Forward,
                "{reply:?}"
            );
        }
        assert_eq!(decide_instance(Err(())), InstanceDecision::Standalone);
    }
}
