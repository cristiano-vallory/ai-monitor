//! Commands arriving at the app while a window may or may not exist.
//! The decision is pure so the whole domain is unit-testable.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Open(u8),
    Hide,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    OpenWindow(u8),
    FocusAndNavigate(u8),
    CloseWindow,
    Exit,
    Ignore,
}

pub fn decide(window_open: bool, cmd: Command) -> Action {
    match cmd {
        Command::Quit => Action::Exit,
        Command::Hide => {
            if window_open {
                Action::CloseWindow
            } else {
                Action::Ignore
            }
        }
        Command::Open(nav) => {
            let nav = if nav > 3 { 0 } else { nav };
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
        assert_eq!(decide(false, Command::Hide), Action::Ignore);
        assert_eq!(decide(true, Command::Hide), Action::CloseWindow);
        for nav in 0..=3u8 {
            assert_eq!(
                decide(false, Command::Open(nav)),
                Action::OpenWindow(nav),
                "nav={nav}"
            );
            assert_eq!(
                decide(true, Command::Open(nav)),
                Action::FocusAndNavigate(nav),
                "nav={nav}"
            );
        }
        for nav in [4u8, 200, u8::MAX] {
            assert_eq!(decide(false, Command::Open(nav)), Action::OpenWindow(0));
            assert_eq!(
                decide(true, Command::Open(nav)),
                Action::FocusAndNavigate(0)
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
